//! Tests for control-group and team-key presses, assignment and team walks.

use super::{
    GroupPressAction, KeyModifiers, TEAM_SELECTION_VOICE_POLICY, TeamSelection,
    command_bar_team_press_action, control_group_action_for_input, control_group_press_action,
    live_group_members, schedule_control_group_assignment, selection_voice_recipients,
    team_selection, team_walk_members,
};
use std::time::Duration;

/// The presentation frame of `sim` as `observer` sees it: the drawers
/// read their records' observer answers from the frame's local player.
fn frame_for(
    sim: &crate::sim::world::Simulation,
    observer: Option<crate::sim::intern::InternedId>,
) -> crate::app::presentation::frame::PresentationFrame {
    let mut sim = sim.clone();
    sim.session_mut_for_test()
        .set_current_house_for_test(observer);
    crate::app::presentation::frame::PresentationFrame::of_test(
        &sim,
        crate::rules::ruleset::RuleSet::empty_for_test(),
    )
}

fn mods(shift: bool, ctrl: bool, alt: bool) -> KeyModifiers {
    KeyModifiers { shift, ctrl, alt }
}

const BARE: KeyModifiers = KeyModifiers {
    shift: false,
    ctrl: false,
    alt: false,
};

/// Exercise the ordinary app producer and the authoritative command
/// receiver without advancing unrelated gameplay for input controls.
fn apply_group_assignment(sim: &mut crate::sim::world::Simulation, slot: usize, selected: &[u64]) {
    let owner = sim
        .session()
        .current_house()
        .expect("fixture current House");
    let owner_name = sim.interner().resolve(owner).to_owned();
    schedule_control_group_assignment(sim, &owner_name, slot, selected.to_vec()).unwrap();
    let commands = sim.take_due_commands();
    assert_eq!(commands.len(), 1);
    assert!(sim.apply_command(&owner_name, &commands[0].payload, None));
}

#[test]
fn retail_bunker_docking_forgets_control_group_through_release_and_saved_view() {
    use crate::app::input::state::{MatchInputState, PreparedSavedLocalView};
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::docking::bunker_link::BunkerLink;
    use crate::ui::sidebar::gadget_flash::SidebarGadgetState;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut rules = retail.rules;
    let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let walls = ["NATBNK_A", "NATBNK_B", "NATBNK_A2", "NATBNK_B2"].map(str::to_owned);
    assert_eq!(
        rules.bind_anim_class_assets(&walls, &assets, "TEM", "TEMPERATE"),
        0
    );
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x459315);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.set_input_delay_ticks(0);
    let bunker = sim
        .spawn_object("NATBNK", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object("MTNK", "Americans", 14, 10, 0, &rules)
        .unwrap();
    let reserve = sim
        .spawn_object("MTNK", "Americans", 18, 14, 0, &rules)
        .unwrap();
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    let mut input = MatchInputState::new(Default::default());
    apply_group_assignment(&mut sim, 0, &[tank, bunker]);
    apply_group_assignment(&mut sim, 9, &[reserve]);
    assert_eq!(sim.projected_control_groups()[0], [bunker, tank]);
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session().tick() + 1,
        Command::clicked_mission(
            tank,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(bunker),
        ),
    ));
    let mut runtime = crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources: crate::sim::runtime::SimResources::new(rules).into(),
    };
    for _ in 0..600 {
        let commands = runtime.simulation.take_due_commands();
        let frame = runtime
            .advance_frame(
                &commands,
                33,
                crate::sim::world::TickLane::Ordinary,
                &mut Default::default(),
            )
            .unwrap();
        let sim = &runtime.simulation;
        for output in &frame.lifecycle_outputs {
            input.apply_lifecycle_output(output, Some(owner));
        }
        if sim.entities().get(tank).unwrap().bunker.link() == BunkerLink::Installed(bunker) {
            break;
        }
        assert_eq!(
            sim.projected_control_groups()[0],
            [bunker, tank],
            "approaching retains the group"
        );
    }
    let sim = &runtime.simulation;
    let rules = &runtime.resources.rules;
    assert_eq!(
        sim.entities().get(tank).unwrap().bunker.link(),
        BunkerLink::Installed(bunker)
    );
    // UpdateBunker458E50/459315 clears +214 at installation, before
    // deselection. A recall-time filter would wrongly revive it on exit.
    assert_eq!(sim.entities().get(tank).unwrap().object_group(), -1);
    assert_eq!(sim.projected_control_groups()[0], [bunker]);
    assert_eq!(sim.projected_control_groups()[9], [reserve]);
    let saved = input
        .capture_saved_local_view(&sim, &rules, &SidebarGadgetState::new())
        .unwrap();
    let saved =
        crate::util::save_codec::deserialize(&crate::util::save_codec::serialize(&saved).unwrap())
            .unwrap();
    let mut restored = MatchInputState::new(Default::default());
    PreparedSavedLocalView::new(saved, &sim, &rules)
        .unwrap()
        .commit_into(&mut restored, &mut SidebarGadgetState::new());
    let next_tick = runtime.simulation.session().tick() + 1;
    runtime.simulation.queue_command(CommandEnvelope::new(
        owner,
        next_tick,
        Command::EjectBunker { bunker_id: bunker },
    ));
    for _ in 0..120 {
        let commands = runtime.simulation.take_due_commands();
        let frame = runtime
            .advance_frame(
                &commands,
                33,
                crate::sim::world::TickLane::Ordinary,
                &mut Default::default(),
            )
            .unwrap();
        let sim = &runtime.simulation;
        for output in &frame.lifecycle_outputs {
            restored.apply_lifecycle_output(output, Some(owner));
        }
        if sim.entities().get(tank).unwrap().bunker.link() == BunkerLink::None {
            break;
        }
    }
    let sim = &runtime.simulation;
    assert_eq!(
        sim.entities().get(tank).unwrap().bunker.link(),
        BunkerLink::None
    );
    assert_eq!(
        team_walk_members(Some(&sim), &sim.projected_control_groups()[0]),
        [bunker]
    );
    assert_eq!(sim.entities().get(tank).unwrap().object_group(), -1);
    assert_eq!(sim.projected_control_groups()[9], [reserve]);
}

/// TabClass::Action 0x006D07EC..0x006D0822: an empty team takes the
/// selection, a fully selected one centres without any double tap (extra
/// selected units allowed), and anything less recalls.
#[test]
fn command_bar_team_button_assigns_empty_centres_selected_else_recalls() {
    assert_eq!(
        command_bar_team_press_action(&[], &[7]),
        GroupPressAction::Assign
    );
    assert_eq!(
        command_bar_team_press_action(&[1, 2], &[2, 1, 7]),
        GroupPressAction::Center
    );
    assert_eq!(
        command_bar_team_press_action(&[1, 2], &[1]),
        GroupPressAction::Recall
    );
}

#[test]
fn ctrl_assigns_shift_adds_alt_centers_bare_recalls() {
    let group = [1u64, 2];
    let selected = [1u64, 2];
    assert_eq!(
        control_group_press_action(mods(false, true, false), 0, &group, &selected, None),
        Some(GroupPressAction::Assign)
    );
    assert_eq!(
        control_group_press_action(mods(true, false, false), 0, &group, &selected, None),
        Some(GroupPressAction::AddToSelection)
    );
    assert_eq!(
        control_group_press_action(mods(false, false, true), 0, &group, &selected, None),
        Some(GroupPressAction::Center)
    );
    assert_eq!(
        control_group_press_action(BARE, 0, &group, &selected, None),
        Some(GroupPressAction::Recall)
    );
}

/// Two modifiers at once produce a key value no binding carries, so the
/// press does nothing at all — Ctrl+Shift+digit must not assign.
#[test]
fn two_modifiers_match_no_binding() {
    let group = [1u64];
    assert_eq!(
        control_group_press_action(mods(true, true, false), 0, &group, &group, None),
        None
    );
    assert_eq!(
        control_group_press_action(mods(false, true, true), 0, &group, &group, None),
        None
    );
}

#[test]
fn double_tap_inside_the_window_centers() {
    let group = [1u64, 2, 3];
    assert_eq!(
        control_group_press_action(
            BARE,
            2,
            &group,
            &group,
            Some((2, Duration::from_millis(799)))
        ),
        Some(GroupPressAction::Center)
    );
}

#[test]
fn double_tap_outside_the_window_recalls() {
    let group = [1u64, 2, 3];
    assert_eq!(
        control_group_press_action(
            BARE,
            2,
            &group,
            &group,
            Some((2, Duration::from_millis(800)))
        ),
        Some(GroupPressAction::Recall)
    );
}

#[test]
fn a_different_slot_inside_the_window_recalls() {
    let group = [1u64];
    assert_eq!(
        control_group_press_action(
            BARE,
            3,
            &group,
            &group,
            Some((2, Duration::from_millis(10)))
        ),
        Some(GroupPressAction::Recall)
    );
}

#[test]
fn double_tap_group_action_ignores_ineligible_selected_enemy() {
    use crate::sim::world::Simulation;

    let mut sim = Simulation::new();
    let local = house(&mut sim, "Local", false, true);
    let enemy = house(&mut sim, "Enemy", false, false);
    group_entity(&mut sim, 10, local);
    group_entity(&mut sim, 11, enemy);
    // Member 12 stays in limbo (inside a transport).
    group_entity(&mut sim, 12, local);
    for id in [10, 11] {
        let entity = sim.entity_mut_for_test(id).unwrap();
        entity.lifecycle.set_in_limbo_for_test(false);
    }
    let raw_selected = [10, 11];

    assert_eq!(
        control_group_action_for_input(
            BARE,
            1,
            &[10, 12],
            &raw_selected,
            Some((1, Duration::from_millis(10))),
            Some(&sim),
        ),
        Some(GroupPressAction::Center),
        "the double tap (0x0073123B..0x0073129D) skips the selected ineligible \
         enemy and the team member in limbo"
    );
}

/// The centre arm needs the selection to be *exactly* the group. One extra
/// selected unit outside the group is one of the two bail-outs.
#[test]
fn an_extra_selected_unit_outside_the_group_recalls() {
    let group = [1u64, 2];
    let selected = [1u64, 2, 9];
    assert_eq!(
        control_group_press_action(
            BARE,
            1,
            &group,
            &selected,
            Some((1, Duration::from_millis(100)))
        ),
        Some(GroupPressAction::Recall)
    );
}

/// And the other bail-out: a group member that is not selected.
#[test]
fn a_group_member_left_unselected_recalls() {
    let group = [1u64, 2, 3];
    let selected = [1u64, 2];
    assert_eq!(
        control_group_press_action(
            BARE,
            1,
            &group,
            &selected,
            Some((1, Duration::from_millis(100)))
        ),
        Some(GroupPressAction::Recall)
    );
}

/// An empty group never centres, however fast the taps come.
#[test]
fn an_empty_group_recalls_rather_than_centering() {
    assert_eq!(
        control_group_press_action(BARE, 4, &[], &[], Some((4, Duration::from_millis(10)))),
        Some(GroupPressAction::Recall)
    );
}

/// Membership is one group index per object, so assigning evicts.
#[test]
fn assignment_evicts_the_units_from_their_previous_group() {
    let mut sim = crate::sim::world::Simulation::new();
    let local = house(&mut sim, "Local", false, true);
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(local));
    for id in 1..=4 {
        group_entity(&mut sim, id, local);
        sim.entity_mut_for_test(id)
            .unwrap()
            .set_object_group_for_test(if id == 4 { 0 } else { 9 });
    }
    apply_group_assignment(&mut sim, 1, &[2, 4]);
    assert_eq!(sim.projected_control_groups()[0], [1, 3]);
    assert_eq!(sim.projected_control_groups()[1], [2, 4]);
}

fn house(
    sim: &mut crate::sim::world::Simulation,
    name: &str,
    is_human: bool,
    player_control: bool,
) -> crate::sim::intern::InternedId {
    let id = sim.intern(name);
    let mut house = crate::sim::house_state::HouseState::new(id, 0, None, is_human, 0, 10);
    house.set_player_control_for_test(player_control);
    sim.houses_mut_for_test().register(house);
    id
}

fn group_entity(
    sim: &mut crate::sim::world::Simulation,
    stable_id: u64,
    owner: crate::sim::intern::InternedId,
) {
    typed_group_entity(sim, stable_id, owner, "MTNK");
}

fn typed_group_entity(
    sim: &mut crate::sim::world::Simulation,
    stable_id: u64,
    owner: crate::sim::intern::InternedId,
    type_name: &str,
) {
    let type_ref = sim.intern(type_name);
    let entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
        stable_id,
        10,
        10,
        0,
        0,
        owner,
        crate::sim::combat::Health::new(100),
        type_ref,
        crate::map::entities::EntityCategory::Unit,
        0,
        5,
        false,
    );
    sim.entities_mut_for_test().insert(entity);
}

#[test]
fn team_assignment_and_recall_apply_owner_eligibility_without_erasing_latent_members() {
    use crate::sim::world::Simulation;

    let mut sim = Simulation::new();
    let local = house(&mut sim, "Local", false, true);
    let ai = house(&mut sim, "AI", false, false);
    let human_only = house(&mut sim, "HumanOnly", true, false);
    let missing_house = sim.intern("MissingHouse");
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(local));
    for (stable_id, owner) in [
        (1, local),
        (2, local),
        (3, local),
        (4, human_only),
        (5, ai),
        (6, missing_house),
        (7, ai),
    ] {
        group_entity(&mut sim, stable_id, owner);
    }
    assert!(
        sim.house_is_human_player(human_only),
        "the campaign IsHumanPlayer predicate admits this owner"
    );
    assert!(
        !sim.control_projection().can_select(human_only),
        "TeamAssign's mode-zero PlayerControl gate excludes a human-flag-only house"
    );

    for (id, group) in [(1, 0), (2, 0), (5, 2), (7, 1)] {
        sim.entity_mut_for_test(id)
            .unwrap()
            .set_object_group_for_test(group);
    }
    sim.change_owner(2, ai);

    apply_group_assignment(&mut sim, 1, &[3, 4, 5, 6]);
    assert_eq!(
        sim.projected_control_groups()[1],
        vec![2, 3],
        "eligible old membership clears, the transferred AI member stays latent, and only the local selection is added"
    );
    assert_eq!(
        sim.projected_control_groups()[3],
        vec![5],
        "an ineligible selected member remains in its prior group"
    );
    assert_eq!(
        live_group_members(Some(&sim), &sim.projected_control_groups()[1]),
        vec![3],
        "recall filters the latent member while its owner is an AI house"
    );

    sim.entity_mut_for_test(1)
        .unwrap()
        .set_object_group_for_test(1);
    apply_group_assignment(&mut sim, 2, &[]);
    assert_eq!(
        sim.projected_control_groups()[2],
        vec![7],
        "an empty assignment clears eligible old members but leaves an ineligible member's native slot intact"
    );

    sim.change_owner(2, local);
    assert_eq!(
        live_group_members(Some(&sim), &sim.projected_control_groups()[1]),
        vec![2, 3],
        "the native group slot becomes usable again when its member returns to an eligible owner"
    );
}

/// Original7310D0 tests the same House gate as731060 before clearing
/// matching+214. An object that changed owner keeps its latent slot;
/// deleting the object removes the field rather than a ghost membership.
#[test]
fn command_bar_team_clear_preserves_latent_ineligible_members() {
    use crate::sim::world::Simulation;

    for multiplayer in [false, true] {
        let mut sim = Simulation::new();
        let local = house(&mut sim, "Local", false, true);
        let ai = house(&mut sim, "AI", false, false);
        let human_only = house(&mut sim, "HumanOnly", true, false);
        group_entity(&mut sim, 1, local);
        group_entity(&mut sim, 2, ai);
        group_entity(&mut sim, 3, human_only);
        group_entity(&mut sim, 4, local);
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(multiplayer),
        );
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(local));
        for id in 1..=4 {
            sim.entity_mut_for_test(id)
                .unwrap()
                .set_object_group_for_test(if id == 4 { 1 } else { 0 });
        }
        let rng_before = sim.rng_state();

        // This is the actual command-bar mutation endpoint, retaining
        // the production digit1 -> Rust slot1 mapping.
        apply_group_assignment(&mut sim, 1, &[]);
        assert_eq!(
            sim.projected_control_groups()[1],
            [2, 3],
            "native7310FD/731105 skips ineligible owners in mode{multiplayer}"
        );
        assert_eq!(
            sim.projected_control_groups()[2],
            [4],
            "clearing1 cannot alter another slot"
        );
        assert_eq!(sim.rng_state(), rng_before, "group clear draws no RNG");
        assert!(live_group_members(Some(&sim), &sim.projected_control_groups()[1]).is_empty());

        sim.change_owner(2, local);
        assert_eq!(
            live_group_members(Some(&sim), &sim.projected_control_groups()[1]),
            [2],
            "the original membership is usable after the owner becomes eligible again"
        );
    }
}

#[test]
fn team_assign_and_command_bar_clear_match_original_assignment_controls() {
    let native =
        crate::test_fixture::json("tools/procedural_drawing_oracle/control_group_status.json");
    let controls = native["assignment_controls"].as_array().unwrap();
    assert!(!controls.is_empty());
    for row in controls {
        let input = &row["input"];
        let mut sim = crate::sim::world::Simulation::new();
        let owners = [
            house(&mut sim, "CurrentHouse", false, false),
            house(&mut sim, "OtherHouse", false, false),
        ];
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
                input["mode"].as_u64().unwrap() != 0,
            ),
        );
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owners[0]));
        let actors = input["actors"].as_array().unwrap();
        let mut selected = Vec::new();
        for (index, actor) in actors.iter().enumerate() {
            let id = index as u64 + 1;
            let owner = owners[actor["owner"].as_u64().unwrap() as usize];
            sim.houses_mut_for_test()
                .get_mut(&owner)
                .unwrap()
                .set_player_control_for_test(actor["player_control"].as_bool().unwrap());
            group_entity(&mut sim, id, owner);
            let is_selected = actor["selected"].as_bool().unwrap();
            sim.entity_mut_for_test(id)
                .unwrap()
                .seldom
                .selection
                .set_selected_for_test(is_selected);
            if is_selected {
                selected.push(id);
            }
            sim.entity_mut_for_test(id)
                .unwrap()
                .set_object_group_for_test(actor["group_slot"].as_i64().unwrap() as i32);
        }
        let fields = |sim: &crate::sim::world::Simulation| {
            (1..=actors.len() as u64)
                .map(|id| sim.entities().get(id).unwrap().object_group())
                .collect::<Vec<_>>()
        };
        for step in row["steps"].as_array().unwrap() {
            assert_eq!(serde_json::json!(fields(&sim)), step["before"], "{row}");
            let command = &step["command"];
            let parameter = command["group"].as_u64().unwrap() as usize;
            assert!((1..=10).contains(&parameter));
            let slot = parameter % 10;
            let rng_before = sim.rng_state();
            match command["operation"].as_str().unwrap() {
                "assign" => apply_group_assignment(&mut sim, slot, &selected),
                "clear" => apply_group_assignment(&mut sim, slot, &[]),
                other => panic!("unsupported original assignment operation {other}"),
            }
            assert_eq!(
                serde_json::json!(fields(&sim)),
                step["after"],
                "{row}: {step}"
            );
            assert_eq!(
                sim.rng_state(),
                rng_before,
                "native assignment draws no RNG"
            );
            assert_eq!(step["timers_unchanged"], true);
        }
        assert!(row["rng_entries"].as_array().unwrap().is_empty());
    }
}

#[test]
fn shift_group_add_drains_a_selected_enemy() {
    use crate::sim::world::Simulation;

    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str("[VehicleTypes]\n0=MTNK\n[MTNK]\n"),
    )
    .expect("group fixture rules");
    let mut sim = Simulation::new();
    let local = house(&mut sim, "Local", false, true);
    let enemy = house(&mut sim, "Enemy", false, false);
    group_entity(&mut sim, 10, local);
    group_entity(&mut sim, 11, enemy);
    for id in [10, 11] {
        let entity = sim.entity_mut_for_test(id).unwrap();
        entity.lifecycle.set_in_limbo_for_test(false);
    }

    let live_group = live_group_members(Some(&sim), &[10, 11]);
    assert_eq!(live_group, [10], "only eligible group members are recalled");
    // AddTeam (0x007314C0) clears nothing and Selects each eligible
    // member (vt+0x14C at 0x0073156C); the click-selected enemy is
    // current-objects [0], so that Select's Unselect_All
    // (0x005F45AF..0x005F45F8) drains it.
    let calls = team_walk_members(Some(&sim), &[10, 11]);
    assert_eq!(calls, [10]);
    let effects = super::resolve_selection_mutation(
        &sim,
        Some(&rules),
        vec![11],
        None,
        false,
        false,
        &super::SelectionMutation {
            select: calls,
            ..Default::default()
        },
    );
    assert_eq!(effects.ordered, [10]);
    assert_eq!(effects.successful_adds, [10]);
}

/// Recall `0x007311C0` walks the Techno array, so the oldest member
/// speaks (latch clear at `0x00731369`) whatever order the team was
/// assigned in, and Select still prepends the armed one (`0x005F468C`).
#[test]
fn team_recall_speaks_the_oldest_member_in_techno_array_order() {
    use crate::sim::world::Simulation;

    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n1=TRUK\n2=HIDE\n[MTNK]\nPrimary=Gun\n[Gun]\nDamage=10\n[TRUK]\n[HIDE]\nSelectable=no\n",
        ),
    )
    .expect("team fixture rules");
    let mut sim = Simulation::new();
    let local = house(&mut sim, "Local", false, true);
    typed_group_entity(&mut sim, 10, local, "TRUK");
    typed_group_entity(&mut sim, 11, local, "MTNK");
    typed_group_entity(&mut sim, 9, local, "HIDE");
    for id in [9, 10, 11] {
        let entity = sim.entity_mut_for_test(id).unwrap();
        entity.lifecycle.set_in_limbo_for_test(false);
    }
    let recall = |group: &[u64]| {
        let plan = team_selection(Some(&sim), GroupPressAction::Recall, group, &[])
            .expect("Recall always clears");
        let effects = super::resolve_selection_mutation(
            &sim,
            Some(&rules),
            vec![],
            None,
            false,
            false,
            &super::SelectionMutation {
                clear: plan.clear,
                select: plan.select,
                ..Default::default()
            },
        );
        let voices = selection_voice_recipients(
            TEAM_SELECTION_VOICE_POLICY,
            true,
            &effects.successful_adds,
            effects.first_select_adds,
            effects.first_attempt_added,
        )
        .to_vec();
        (effects.ordered, voices)
    };
    // Assigned newest-armed first, as a prepended selection stores it.
    assert_eq!(recall(&[11, 10]), (vec![11, 10], vec![10]));
    // A refused first Select (Selectable=no) still clears the latch.
    assert_eq!(recall(&[11, 10, 9]), (vec![11, 10], vec![]));
}

/// CenterCamera `0x007313A0` keeps an already-selected team led by one of
/// its Foot members and Selects nothing, so Alt+digit queues nothing and
/// stays silent; an outsider or a team building first in the selection
/// (not a Foot, `0x007313F8..0x00731404`) clears it first.
#[test]
fn center_camera_keeps_a_selected_team_unless_an_outsider_or_building_leads() {
    use crate::sim::world::Simulation;

    let mut sim = Simulation::new();
    let local = house(&mut sim, "Local", false, true);
    for id in [10, 11, 77] {
        group_entity(&mut sim, id, local);
    }
    let power = sim.intern("GAPOWR");
    sim.entities_mut_for_test().insert(
        crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
            5,
            12,
            12,
            0,
            0,
            local,
            crate::sim::combat::Health::new(100),
            power,
            crate::map::entities::EntityCategory::Structure,
            0,
            5,
            false,
        ),
    );
    for id in [5, 10, 11, 77] {
        let entity = sim.entity_mut_for_test(id).unwrap();
        entity.lifecycle.set_in_limbo_for_test(false);
    }
    let group = [10, 5, 11];
    let center =
        |selected: &[u64]| team_selection(Some(&sim), GroupPressAction::Center, &group, selected);
    assert_eq!(center(&[10, 5, 11, 77]), None);
    let reselect = Some(TeamSelection {
        clear: true,
        select: vec![5, 10, 11],
    });
    assert_eq!(center(&[77, 10]), reselect);
    assert_eq!(center(&[5, 10, 11]), reselect);
}

#[test]
fn multiplayer_group_eligibility_is_the_current_house_not_house_flags() {
    use crate::sim::world::Simulation;

    let mut sim = Simulation::new();
    let local = house(&mut sim, "Local", false, false);
    let other_human = house(&mut sim, "OtherHuman", true, true);
    group_entity(&mut sim, 10, local);
    group_entity(&mut sim, 11, other_human);
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(local));

    assert_eq!(
        live_group_members(Some(&sim), &[10, 11]),
        vec![10],
        "multiplayer recall follows IsHumanPlayer's current-house comparison"
    );
}

#[test]
fn registered_group_assignment_projects_a_selected_building_status_badge() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;

    let mut sim = Simulation::new();
    let owner = house(&mut sim, "Americans", false, true);
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    let building_type = sim.intern("YAPOWR");
    let mut building = GameEntity::new_at_frame_zero_for_test(
        27,
        12,
        10,
        0,
        0,
        owner,
        Health::new(1000),
        building_type,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    building.seldom.selection.set_selected_for_test(true);
    sim.entities_mut_for_test().insert(building);
    assert!(matches!(
        sim.reveal(27),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    assert!(
        crate::app::presentation::instances::tactical_entity_encounter_order(&sim, None)
            .contains(&27)
    );

    let create_one_slot = crate::app::input::hotkeys::catalog::registered_commands()
        .iter()
        .find(|metadata| metadata.ini_name == "TeamCreate_1")
        .map(|metadata| metadata.command)
        .expect("registered TeamCreate_1 command");
    let crate::app::input::hotkeys::HotkeyCommand::TeamCreate(slot) = create_one_slot else {
        panic!("TeamCreate_1 must remain the existing group assignment command");
    };
    schedule_control_group_assignment(&mut sim, "Americans", slot, vec![27]).unwrap();
    assert_eq!(sim.entities().get(27).unwrap().object_group(), -1);
    let groups = sim.projected_control_groups();

    assert_eq!(
        crate::app::presentation::ui_overlays::control_group_status_labels(
            &sim,
            &frame_for(&sim, Some(owner)),
            &groups,
            None,
            Some(owner),
            false,
            None,
        ),
        vec![(27, '1')],
        "a retained, selected YAPOWR follows the same live group projection"
    );
}

#[test]
fn registered_aircraft_group_badge_uses_shared_screen_height_and_visibility() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;

    let mut sim = Simulation::new();
    let owner = sim.intern("Americans");
    let mut aircraft = GameEntity::new_at_frame_zero_for_test(
        27,
        12,
        10,
        0,
        0,
        owner,
        Health::new(100),
        sim.intern("ORCA"),
        EntityCategory::Aircraft,
        0,
        5,
        true,
    );
    aircraft.position.set_exact_z_leptons_for_test(Some(0));
    aircraft.seldom.selection.set_selected_for_test(true);
    sim.entities_mut_for_test().insert(aircraft);
    assert!(matches!(
        sim.reveal(27),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    assert!(
        crate::app::presentation::instances::tactical_entity_encounter_order(&sim, None)
            .contains(&27)
    );

    let registered_slot = |name: &str| {
        let command = crate::app::input::hotkeys::catalog::registered_commands()
            .iter()
            .find(|metadata| metadata.ini_name == name)
            .unwrap_or_else(|| panic!("missing registered hotkey {name}"))
            .command;
        match command {
            crate::app::input::hotkeys::HotkeyCommand::TeamCreate(slot) => slot,
            command => panic!("unexpected command for {name}: {command:?}"),
        }
    };
    let mut groups = vec![Vec::new(); 10];
    groups[registered_slot("TeamCreate_1")] = vec![27];

    let font =
        crate::render::bit_font::tests::make_test_font(&[(b'0' as u16, 6), (b'1' as u16, 5)], 4);
    let house_colors = crate::map::houses::HouseColorMap::new();
    let house_color_ramps = crate::rules::house_colors::HouseColorRamps::default();
    let render = |sim: &Simulation, groups: &[Vec<u64>], hovered| {
        crate::app::presentation::ui_overlays::build_control_group_status_render_instances(
            sim,
            &frame_for(sim, Some(owner)),
            groups,
            hovered,
            Some(owner),
            false,
            &house_colors,
            &house_color_ramps,
            &font,
            [0.0, 0.0],
            1600.0,
            1200.0,
            [0, 0, 1600, 1200],
            None,
        )
    };

    let ground_anchor = crate::render::locomotor_visual::screen_position(
        sim.entities().get(27).expect("retained ORCA"),
    );
    let selected_ground = render(&sim, &groups, None);
    assert_eq!(selected_ground.len(), 1);
    let selected_ground = selected_ground.get(&27).expect("ORCA's admitted label");
    assert_eq!(selected_ground.text.len(), 1);
    assert_eq!(selected_ground.rectangles.len(), 5);
    assert_eq!(selected_ground.rectangles[0].tint, [0.0; 3]);
    let ground_text_position = selected_ground.text[0].position;

    {
        let aircraft = sim.entity_mut_for_test(27).expect("retained ORCA");
        aircraft.seldom.selection.set_selected_for_test(false);
        aircraft.position.set_exact_z_leptons_for_test(Some(1500));
    }
    let airborne_anchor = crate::render::locomotor_visual::screen_position(
        sim.entities().get(27).expect("retained ORCA"),
    );
    let hovered_airborne = render(&sim, &groups, Some(27));
    assert_eq!(hovered_airborne.len(), 1);
    let hovered_airborne = hovered_airborne.get(&27).expect("hovered ORCA's label");
    assert_eq!(hovered_airborne.text.len(), 1);
    let airborne_text_position = hovered_airborne.text[0].position;
    assert_eq!(airborne_text_position[0], ground_text_position[0]);
    assert!(airborne_text_position[1] < ground_text_position[1]);
    assert!(
        ((airborne_text_position[1] - ground_text_position[1])
            - (airborne_anchor.1 - ground_anchor.1))
            .abs()
            < 0.001,
        "Aircraft badge follows the shared exact-Z screen-position lift"
    );

    groups[1].clear();
    groups[registered_slot("TeamCreate_10")] = vec![27];
    assert_eq!(
        crate::app::presentation::ui_overlays::control_group_status_labels(
            &sim,
            &frame_for(&sim, Some(owner)),
            &groups,
            Some(27),
            Some(owner),
            false,
            None,
        ),
        vec![(27, '0')]
    );
    assert_eq!(render(&sim, &groups, Some(27))[&27].text.len(), 1);

    sim.entity_mut_for_test(27)
        .expect("retained ORCA")
        .seldom
        .selection
        .set_selected_for_test(false);
    assert!(render(&sim, &groups, None).is_empty());
    groups[registered_slot("TeamCreate_10")].clear();
    sim.entity_mut_for_test(27)
        .expect("retained ORCA")
        .seldom
        .selection
        .set_selected_for_test(true);
    assert!(render(&sim, &groups, None).is_empty());
}

#[test]
fn building_group_badges_follow_registered_slots_visibility_and_removal() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;

    let mut sim = Simulation::new();
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    let current_house = sim.intern("Americans");
    let allied_house = sim.intern("French");
    let gap_house = sim.intern("Russians");
    let alliances = crate::map::houses::HouseAllianceMap::from([
        (
            "AMERICANS".to_owned(),
            std::collections::BTreeSet::from(["FRENCH".to_owned()]),
        ),
        (
            "FRENCH".to_owned(),
            std::collections::BTreeSet::from(["AMERICANS".to_owned()]),
        ),
    ]);
    *sim.house_alliances_mut_for_test() = alliances;

    // Body/status admission is the fixture subject. Supply no sustained
    // sight source: mark_visible below intentionally tests transient
    // visibility that a hostile Gap can conceal while Display is retained.
    let mut building = GameEntity::new_at_frame_zero_for_test(
        27,
        12,
        10,
        0,
        0,
        allied_house,
        Health::new(1000),
        sim.intern("YAPOWR"),
        EntityCategory::Structure,
        0,
        0,
        false,
    );
    building
        .seldom
        .type_facts
        .set_foundation_for_test("2x2".to_owned().into());
    building.seldom.selection.set_selected_for_test(true);
    sim.entities_mut_for_test().insert(building);
    assert!(matches!(
        sim.reveal(27),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));

    let registered_slot = |name: &str| {
        let command = crate::app::input::hotkeys::catalog::registered_commands()
            .iter()
            .find(|metadata| metadata.ini_name == name)
            .unwrap_or_else(|| panic!("missing registered hotkey {name}"))
            .command;
        match command {
            crate::app::input::hotkeys::HotkeyCommand::TeamCreate(slot) => slot,
            command => panic!("unexpected command for {name}: {command:?}"),
        }
    };
    let mut groups = vec![Vec::new(); 10];
    groups[registered_slot("TeamCreate_1")] = vec![27];

    fn labels(
        sim: &Simulation,
        groups: &[Vec<u64>],
        current_house: crate::sim::intern::InternedId,
        hovered: Option<u64>,
        ignore_visibility: bool,
    ) -> Vec<(u64, char)> {
        crate::app::presentation::ui_overlays::control_group_status_labels(
            sim,
            &frame_for(sim, Some(current_house)),
            groups,
            hovered,
            Some(current_house),
            ignore_visibility,
            None,
        )
    }
    assert!(labels(&sim, &groups, current_house, None, false).is_empty());
    sim.fog_mut_for_test()
        .mark_visible_for_owner(current_house, 12, 10);
    assert_eq!(
        labels(&sim, &groups, current_house, None, false),
        vec![(27, '1')]
    );

    sim.entity_mut_for_test(27)
        .expect("retained building")
        .seldom
        .selection
        .set_selected_for_test(false);
    assert!(labels(&sim, &groups, current_house, None, false).is_empty());
    assert_eq!(
        labels(&sim, &groups, current_house, Some(27), false),
        vec![(27, '1')]
    );

    let (alliances, interner) = (sim.house_alliances().clone(), sim.interner().clone());

    crate::sim::vision::apply_gap_generators(
        sim.fog_mut_for_test(),
        &alliances,
        &[(gap_house, 12, 10, 5)],
        &interner,
    );
    assert!(labels(&sim, &groups, current_house, Some(27), false).is_empty());
    assert_eq!(
        labels(&sim, &groups, current_house, Some(27), true),
        vec![(27, '1')]
    );

    sim.entity_mut_for_test(27)
        .expect("retained building")
        .seldom
        .selection
        .set_selected_for_test(true);
    groups[1].clear();
    groups[registered_slot("TeamCreate_10")] = vec![27];
    assert_eq!(
        labels(&sim, &groups, current_house, None, true),
        vec![(27, '0')]
    );
    groups[0].clear();
    assert!(labels(&sim, &groups, current_house, None, true).is_empty());

    groups[1] = vec![27];
    sim.entities_mut_for_test().remove(27);
    assert!(labels(&sim, &groups, current_house, None, true).is_empty());
}

#[test]
fn status_group_projection_follows_group_changes_and_removal() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;

    let mut sim = Simulation::new();
    let owner = sim.intern("Americans");
    let enemy_owner = sim.intern("Russians");
    let unit_type = sim.intern("MTNK");
    let mut unit = GameEntity::new_at_frame_zero_for_test(
        17,
        10,
        10,
        0,
        0,
        owner,
        Health::new(100),
        unit_type,
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    unit.seldom.selection.set_selected_for_test(true);
    sim.entities_mut_for_test().insert(unit);
    assert!(matches!(
        sim.reveal(17),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    let unselected_unit = GameEntity::new_at_frame_zero_for_test(
        18,
        11,
        10,
        0,
        0,
        owner,
        Health::new(100),
        unit_type,
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    sim.entities_mut_for_test().insert(unselected_unit);
    assert!(matches!(
        sim.reveal(18),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    let mut enemy = GameEntity::new_at_frame_zero_for_test(
        19,
        12,
        10,
        0,
        0,
        enemy_owner,
        Health::new(100),
        unit_type,
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    enemy.seldom.selection.set_selected_for_test(true);
    sim.entities_mut_for_test().insert(enemy);
    assert!(matches!(
        sim.reveal(19),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    // The native pip tail admits non-allies only through Techno+0x210.
    // That bit is not modeled; a shroud-revealed enemy still does not take
    // the common CurrentPlayer-allied path implemented here.
    sim.fog_mut_for_test().mark_visible_for_owner(owner, 12, 10);
    let display_order =
        crate::app::presentation::instances::tactical_entity_encounter_order(&sim, None);
    for id in [17, 18, 19] {
        assert!(
            display_order.contains(&id),
            "fixture {id} must use the retained production Display list"
        );
    }

    // These transient render fixtures contain the same ten registered-digit
    // slots as Simulation::projected_control_groups. Assignment behavior
    // is exercised through the simulation command owner above.
    let mut groups = vec![Vec::new(); 10];
    let projected_with_visibility = |groups: &[Vec<u64>], hovered_unit_id, ignore_visibility| {
        crate::app::presentation::ui_overlays::control_group_status_labels(
            &sim,
            &frame_for(&sim, Some(owner)),
            groups,
            hovered_unit_id,
            Some(owner),
            ignore_visibility,
            None,
        )
    };
    let projected = |groups: &[Vec<u64>], hovered_unit_id| {
        projected_with_visibility(groups, hovered_unit_id, false)
    };
    let font = crate::render::bit_font::tests::make_test_font(
        &[(b'0' as u16, 6), (b'1' as u16, 5), (b'2' as u16, 7)],
        4,
    );
    let house_colors = crate::map::houses::HouseColorMap::new();
    let house_color_ramps = crate::rules::house_colors::HouseColorRamps::default();
    let group_member_anchor = crate::render::locomotor_visual::screen_position(
        sim.entities().get(17).expect("assigned test unit"),
    );
    let fixture_clip = [
        group_member_anchor.0.floor() as i32 - 100,
        group_member_anchor.1.floor() as i32 - 100,
        200,
        200,
    ];
    let status_text_left = group_member_anchor.0 - 14.0 - 3.0;
    let status_text_top = group_member_anchor.1 - 0x1A as f32 + 2.0;
    assert!(
        status_text_left >= fixture_clip[0] as f32
            && status_text_left + 12.0 <= (fixture_clip[0] + fixture_clip[2]) as f32
            && status_text_top >= fixture_clip[1] as f32
            && status_text_top + 13.0 <= (fixture_clip[1] + fixture_clip[3]) as f32,
        "the synthetic render clip contains the projected unit's whole status frame"
    );
    let group_overlay = |groups: &[Vec<u64>], hovered_unit_id| {
        crate::app::presentation::ui_overlays::build_control_group_status_render_instances(
            &sim,
            &frame_for(&sim, Some(owner)),
            groups,
            hovered_unit_id,
            Some(owner),
            false,
            &house_colors,
            &house_color_ramps,
            &font,
            [0.0, 0.0],
            800.0,
            600.0,
            fixture_clip,
            None,
        )
    };
    let group_glyphs = |groups: &[Vec<u64>], hovered_unit_id| {
        group_overlay(groups, hovered_unit_id)
            .into_values()
            .flat_map(|parent| parent.text)
            .collect::<Vec<_>>()
    };
    let registered_create_slot = |name: &str| {
        let command = crate::app::input::hotkeys::catalog::registered_commands()
            .iter()
            .find(|metadata| metadata.ini_name == name)
            .unwrap_or_else(|| panic!("missing registered hotkey {name}"))
            .command;
        match command {
            crate::app::input::hotkeys::HotkeyCommand::TeamCreate(slot) => slot,
            command => panic!("unexpected command for {name}: {command:?}"),
        }
    };

    let create_one_slot = registered_create_slot("TeamCreate_1");
    assert_eq!(create_one_slot, 1);
    groups[create_one_slot] = vec![17, 18, 19];
    assert_eq!(projected(&groups, None), vec![(17, '1')]);
    let group_one = group_overlay(&groups, None);
    assert_eq!(group_one.len(), 1);
    let group_one = group_one.get(&17).expect("assigned unit's group1 label");
    assert_eq!(group_one.text.len(), 1);
    assert_eq!(group_one.rectangles.len(), 5);
    assert_eq!(group_one.rectangles[0].tint, [0.0; 3]);
    assert_eq!(projected(&groups, Some(17)), vec![(17, '1')]);
    let first_glyph = group_glyphs(&groups, Some(17));
    assert_eq!(first_glyph.len(), 1);
    let hovered_labels = projected(&groups, Some(18));
    assert!(hovered_labels.contains(&(17, '1')));
    assert!(hovered_labels.contains(&(18, '1')));
    assert_eq!(
        group_overlay(&groups, Some(18))
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        [17, 18],
        "each admitted selected/hovered parent retains its own label"
    );
    assert_eq!(group_glyphs(&groups, Some(18)).len(), 2);
    assert_eq!(
        projected(&groups, Some(19)),
        vec![(17, '1')],
        "the pip-only allied gate excludes a revealed non-allied unit"
    );

    groups[1].clear();
    groups[2] = vec![17, 18, 19];
    assert_eq!(projected(&groups, None), vec![(17, '2')]);
    let group_two = group_overlay(&groups, None);
    assert_eq!(group_two.len(), 1);
    let group_two = group_two.get(&17).expect("assigned unit's group2 label");
    assert_eq!(group_two.text.len(), 1);
    assert_eq!(group_two.rectangles.len(), 5);
    assert_eq!(group_two.rectangles[0].tint, [0.0; 3]);

    let create_zero_slot = registered_create_slot("TeamCreate_10");
    assert_eq!(create_zero_slot, 0);
    groups[2] = vec![18, 19];
    groups[create_zero_slot] = vec![17];
    assert_eq!(projected(&groups, None), vec![(17, '0')]);
    let group_zero = group_overlay(&groups, None);
    assert_eq!(group_zero.len(), 1);
    let group_zero = group_zero.get(&17).expect("assigned unit's group0 label");
    assert_eq!(group_zero.text.len(), 1);
    assert_eq!(group_zero.rectangles.len(), 5);
    assert_eq!(group_zero.rectangles[0].tint, [0.0; 3]);

    groups[0].clear();
    assert!(projected(&groups, None).is_empty());
    let no_group_overlay = group_overlay(&groups, None);
    assert!(no_group_overlay.is_empty());

    groups[0] = vec![17];
    sim.entities_mut_for_test().remove(17);
    assert!(
        crate::app::presentation::ui_overlays::control_group_status_labels(
            &sim,
            &frame_for(&sim, Some(owner)),
            &groups,
            None,
            Some(owner),
            false,
            None,
        )
        .is_empty()
    );
    assert!(
        crate::app::presentation::ui_overlays::build_control_group_status_render_instances(
            &sim,
            &frame_for(&sim, Some(owner)),
            &groups,
            None,
            Some(owner),
            false,
            &house_colors,
            &house_color_ramps,
            &font,
            [0.0, 0.0],
            800.0,
            600.0,
            [0, 0, 800, 600],
            None,
        )
        .is_empty()
    );
}

#[test]
fn allied_control_group_labels_follow_body_admission_without_raw_anchor_gate() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;

    fn labels(
        sim: &Simulation,
        groups: &[Vec<u64>],
        current_house: crate::sim::intern::InternedId,
        ignore_visibility: bool,
    ) -> Vec<(u64, char)> {
        crate::app::presentation::ui_overlays::control_group_status_labels(
            sim,
            &frame_for(sim, Some(current_house)),
            groups,
            None,
            Some(current_house),
            ignore_visibility,
            None,
        )
    }

    let mut sim = Simulation::new();
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    let current_house = sim.intern("Americans");
    let ally = sim.intern("French");
    let gap_owner = sim.intern("Russians");
    let allies = crate::map::houses::HouseAllianceMap::from([
        (
            "AMERICANS".to_owned(),
            std::collections::BTreeSet::from(["FRENCH".to_owned()]),
        ),
        (
            "FRENCH".to_owned(),
            std::collections::BTreeSet::from(["AMERICANS".to_owned()]),
        ),
    ]);
    *sim.house_alliances_mut_for_test() = allies;

    // Body/status admission is the fixture subject. Supply no sustained
    // sight source: mark_visible below intentionally tests transient
    // visibility that a hostile Gap can conceal while Display is retained.
    let mut unit = GameEntity::new_at_frame_zero_for_test(
        80,
        10,
        10,
        0,
        0,
        ally,
        Health::new(100),
        sim.intern("MTNK"),
        EntityCategory::Unit,
        0,
        0,
        false,
    );
    unit.seldom.selection.set_selected_for_test(true);
    sim.entities_mut_for_test().insert(unit);
    assert!(matches!(
        sim.reveal(80),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    let mut groups = vec![Vec::new(); 10];
    groups[1] = vec![80];
    assert!(
        crate::app::presentation::instances::tactical_entity_encounter_order(&sim, None)
            .contains(&80),
        "the allied unit remains in the retained Display projection"
    );

    // Native Extras6D907E/6D9111 ->6D95D2..9789 ->6F5E27..87
    // reaches the shared pip tail from selected retained body admission;
    // the group tail70A6FD adds no independent raw-anchor fog query.
    assert_eq!(labels(&sim, &groups, current_house, false), vec![(80, '1')]);
    assert_eq!(
        labels(&sim, &groups, current_house, true),
        vec![(80, '1')],
        "sandbox does not change the represented body/pip relationship policy"
    );

    sim.fog_mut_for_test()
        .mark_visible_for_owner(current_house, 10, 10);
    assert!(sim.fog().is_cell_revealed(current_house, 10, 10));
    assert_eq!(labels(&sim, &groups, current_house, false), vec![(80, '1')]);

    let (alliances, interner) = (sim.house_alliances().clone(), sim.interner().clone());

    crate::sim::vision::apply_gap_generators(
        sim.fog_mut_for_test(),
        &alliances,
        &[(gap_owner, 10, 10, 5)],
        &interner,
    );
    assert!(sim.fog().is_cell_gap_covered(current_house, 10, 10));
    assert!(!sim.fog().is_cell_revealed(current_house, 10, 10));
    assert_eq!(
        labels(&sim, &groups, current_house, false),
        vec![(80, '1')],
        "a retained admitted body keeps its label across a raw-anchor Gap frontier"
    );
    assert_eq!(labels(&sim, &groups, current_house, true), vec![(80, '1')]);

    assert!(matches!(
        sim.conceal(80),
        crate::sim::world::ConcealOutcome::Concealed
    ));
    assert!(labels(&sim, &groups, current_house, false).is_empty());
    assert!(
        labels(&sim, &groups, current_house, true).is_empty(),
        "sandbox cannot resurrect a concealed body status tail"
    );
}

#[test]
fn native_tail_corpus_matches_control_group_production_text_builder() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;

    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/control_group_text_native.json",
    ))
    .expect("native control-group text corpus is valid JSON");
    assert_eq!(
        corpus["schema"], "control-group-tail-native-v1",
        "the Rust consumer is tied to the original bounded draw-tail corpus"
    );

    let metrics = corpus["native_font_initialization"]["digit_metrics"]
        .as_array()
        .expect("native digit metrics");
    let glyph_widths: Vec<(u16, u32)> = metrics
        .iter()
        .map(|metric| {
            (
                b'0' as u16 + metric["digit"].as_u64().expect("digit") as u16,
                metric["glyph_width"].as_u64().expect("glyph width") as u32,
            )
        })
        .collect();
    let font = crate::render::bit_font::tests::make_test_font(&glyph_widths, 4);
    let bitmap_rows = corpus["native_font_initialization"]["runtime_fields"]["bitmap_rows"]
        .as_u64()
        .expect("native bitmap rows") as f32;
    let house_rgb: [u8; 3] = corpus["provenance"]["house_rgb_input"]
        .as_array()
        .expect("native House+0x56F9 RGB input")
        .iter()
        .map(|channel| channel.as_u64().expect("house RGB byte") as u8)
        .collect::<Vec<_>>()
        .try_into()
        .expect("three House RGB channels");
    let house_color_native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/house_color.json",
    ))
    .expect("native house-color corpus is valid JSON");
    let house_entries = house_color_native["colors"]
        .as_array()
        .expect("native house colors");
    let schemes: Vec<crate::rules::color_scheme::ColorSchemeEntry> = house_entries
        .iter()
        .map(|entry| crate::rules::color_scheme::ColorSchemeEntry {
            name: entry["name"].as_str().expect("house color name").into(),
            hsv: entry["hsv"]
                .as_array()
                .expect("house color HSV")
                .iter()
                .map(|value| value.as_u64().expect("HSV byte") as u8)
                .collect::<Vec<_>>()
                .try_into()
                .expect("three HSV components"),
        })
        .collect();
    let gold_index = house_entries
        .iter()
        .position(|entry| entry["name"] == "Gold")
        .expect("stock Gold scheme");
    let native_gold_rgb: [u8; 3] = house_entries[gold_index]["modes"]["scalar"]["rgb"]
        .as_array()
        .expect("native Gold House RGB")
        .iter()
        .map(|value| value.as_u64().expect("RGB byte") as u8)
        .collect::<Vec<_>>()
        .try_into()
        .expect("three native RGB channels");
    assert_eq!(house_rgb, native_gold_rgb);
    let house_color_index = crate::rules::house_colors::HouseColorIndex(gold_index as u8);
    let house_color_ramps = crate::rules::house_colors::HouseColorRamps::from_schemes(&schemes);
    let house_colors =
        crate::map::houses::HouseColorMap::from([("Americans".to_owned(), house_color_index)]);
    assert_eq!(
        crate::render::palette_light::house_color_rgb(&house_color_ramps, house_color_index),
        house_rgb,
        "the shared house-color owner resolves native Gold House+0x56F9"
    );

    for row in corpus["cases"].as_array().expect("native cases") {
        let case = &row["case"];
        let label = case["label"].as_str().expect("case label");
        let native_slot = case["slot"].as_u64().expect("native Techno slot") as usize;
        let kind = case["kind"].as_u64().expect("native WhatAmI");
        let digit = row["text"].as_str().expect("native digit");
        let digit_index = digit
            .as_bytes()
            .first()
            .and_then(|value| value.checked_sub(b'0'))
            .expect("native digit index") as usize;
        assert_eq!(
            digit_index,
            if native_slot == 9 { 0 } else { native_slot + 1 },
            "native Techno index maps to the registered Rust digit slot"
        );
        let packed_rgb = row["text_submission"]["packed_owner_rgb"]
            .as_str()
            .expect("native packed house RGB")
            .strip_prefix("0x")
            .and_then(|value| u16::from_str_radix(value, 16).ok())
            .expect("packed RGB565 word");
        assert_eq!(
            crate::render::native_surface_format::RGB565.pack_rgb8(house_rgb),
            packed_rgb,
            "native case {label} packs the direct House+0x56F9 RGB"
        );
        let native_caller_point = case["caller_point"].as_array().expect("caller point");
        let native_x = native_caller_point[0].as_i64().expect("caller x") as f32;
        let native_y = native_caller_point[1].as_i64().expect("caller y") as f32;

        let mut sim = Simulation::new();
        let owner = sim.intern("Americans");
        let category = match kind {
            0xF => EntityCategory::Infantry,
            2 => EntityCategory::Aircraft,
            6 => EntityCategory::Structure,
            _ => EntityCategory::Unit,
        };
        let type_name = case["type_name"].as_str().unwrap_or(match category {
            EntityCategory::Aircraft => "ORCA",
            EntityCategory::Infantry => "E1",
            EntityCategory::Structure => "YAPOWR",
            _ => "MTNK",
        });
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            10,
            0,
            0,
            owner,
            Health::new(100),
            sim.intern(type_name),
            category,
            0,
            5,
            category == EntityCategory::Aircraft,
        );
        if let Some(foundation) = case["foundation"].as_str() {
            entity
                .seldom
                .type_facts
                .set_foundation_for_test(foundation.to_owned().into());
        }
        entity.seldom.selection.set_selected_for_test(true);
        sim.entities_mut_for_test().insert(entity);
        assert!(matches!(
            sim.reveal(1),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));
        let mut groups = vec![Vec::new(); 10];
        groups[digit_index] = vec![1];
        assert_eq!(
            crate::app::presentation::ui_overlays::control_group_status_labels(
                &sim,
                &frame_for(&sim, Some(owner)),
                &groups,
                None,
                Some(owner),
                false,
                None,
            ),
            vec![(1, digit.chars().next().expect("one native digit"))],
            "the assignment reaches the same live production projection"
        );

        let coord = crate::sim::movement::ground_pose::object_get_coords(
            sim.entities().get(1).expect("retained live entity"),
            sim.resolved_terrain(),
        );
        let anchor = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
        // Align the actual GetCoords/integer-projection caller prior to
        // the native tail fixture. Foundation adjustment, measurement,
        // glyph geometry and clipping remain the producer's work.
        let camera = [anchor.0 - native_x, anchor.1 - native_y];
        let (sw, sh) = if label == "outside" {
            (100.0, 100.0)
        } else {
            (640.0, 480.0)
        };
        let overlay_instances =
            crate::app::presentation::ui_overlays::build_control_group_status_render_instances(
                &sim,
                &frame_for(&sim, Some(owner)),
                &groups,
                None,
                Some(owner),
                false,
                &house_colors,
                &house_color_ramps,
                &font,
                camera,
                sw,
                sh,
                case["clip"]
                    .as_array()
                    .expect("native clip xywh")
                    .iter()
                    .map(|value| value.as_i64().expect("clip coordinate") as i32)
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("native clip has four xywh values"),
                None,
            );
        if label == "outside" {
            assert!(
                overlay_instances.is_empty(),
                "the off-viewport native case admits no fill, outline or glyph parent"
            );
            continue;
        }
        assert_eq!(overlay_instances.len(), 1, "one retained native actor");
        let overlay_instances = overlay_instances.get(&1).expect("native actor's label");
        let glyphs = &overlay_instances.text;

        assert_eq!(glyphs.len(), 1, "native case {label} emits one digit quad");
        assert_eq!(
            overlay_instances.rectangles.len(),
            5,
            "native case {label} emits the black fill before its outline and glyph"
        );
        let native_fill = &overlay_instances.rectangles[0];
        assert_eq!(
            native_fill.tint, [0.0; 3],
            "native background_fill color_arg 0 is rendered as black"
        );
        assert_eq!(native_fill.alpha, 1.0);
        let native_fill_record = &row["background_fill"];
        let expected_fill = native_fill_record["rect"]
            .as_array()
            .expect("native clipped fill rectangle")
            .iter()
            .map(|value| value.as_i64().expect("fill coordinate") as f32)
            .collect::<Vec<_>>();
        assert_eq!(
            native_fill_record["color_arg"].as_u64(),
            Some(0),
            "the original fill call uses color_arg 0"
        );
        let native_outline_record = &row["surface_draw"];
        assert_eq!(
            native_outline_record["color_arg"].as_u64(),
            Some(u64::from(packed_rgb)),
            "the original four-line outline uses the packed House RGB"
        );
        for (axis, value) in [
            (0, native_fill.position[0] - camera[0]),
            (1, native_fill.position[1] - camera[1]),
            (2, native_fill.size[0]),
            (3, native_fill.size[1]),
        ] {
            assert_eq!(
                value, expected_fill[axis],
                "native case {label} background fill axis {axis}"
            );
        }
        assert_eq!(overlay_instances.rectangles[1..].len(), 4);
        // The expected packed word is observed at the original surface
        // call. Keep native word extraction and display expansion with
        // their existing format/profile owners, after the House lookup.
        let [red, green, blue] =
            crate::render::native_surface_format::RGB565.unpack_rgb8(packed_rgb);
        let [red, green, blue, _] =
            crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION
                .quantize_rgba8([red, green, blue, 255]);
        let expected_tint = [red, green, blue].map(|channel| f32::from(channel) / 255.0);
        let expected_outline = [
            (
                [expected_fill[0], expected_fill[1]],
                [expected_fill[2], 1.0],
            ),
            (
                [expected_fill[0], expected_fill[1] + expected_fill[3] - 1.0],
                [expected_fill[2], 1.0],
            ),
            (
                [expected_fill[0], expected_fill[1] + 1.0],
                [1.0, expected_fill[3] - 2.0],
            ),
            (
                [
                    expected_fill[0] + expected_fill[2] - 1.0,
                    expected_fill[1] + 1.0,
                ],
                [1.0, expected_fill[3] - 2.0],
            ),
        ];
        for (line, (expected_position, expected_size)) in overlay_instances.rectangles[1..]
            .iter()
            .zip(expected_outline)
        {
            assert_eq!(
                [line.position[0] - camera[0], line.position[1] - camera[1],],
                expected_position,
                "native case {label} outline line position"
            );
            assert_eq!(line.size, expected_size, "native case {label} outline size");
            assert_eq!(
                line.tint, expected_tint,
                "native case {label} outline presents the direct House+0x56F9 packed RGB"
            );
        }
        let expected_width = metrics[digit_index]["glyph_width"]
            .as_u64()
            .expect("native glyph width") as f32;
        assert_eq!(
            glyphs[0].size,
            [expected_width, bitmap_rows],
            "native case {label} uses the pinned GAME.FNT glyph metrics"
        );
        assert_eq!(
            glyphs[0].tint, expected_tint,
            "native case {label} presents House+0x56F9 through the existing RGB owner"
        );
        assert_eq!(
            font.text_width(digit),
            row["font_measure"].as_u64().expect("native text measure") as u32,
            "native case {label} measurement follows the shared BitFont owner"
        );
        let expected = row["text_submission"]["position"]
            .as_array()
            .expect("native text submission position");
        for (axis, value) in [
            (0, glyphs[0].position[0] - camera[0]),
            (1, glyphs[0].position[1] - camera[1]),
        ] {
            let expected = expected[axis].as_i64().expect("native text coordinate") as f32;
            assert!(
                (value - expected).abs() < 0.01,
                "native case {label} axis {axis}: Rust {value} != native {expected}"
            );
        }
    }
}

#[test]
fn assigning_an_empty_selection_empties_the_slot() {
    let mut sim = crate::sim::world::Simulation::new();
    let local = house(&mut sim, "Local", false, true);
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(local));
    for id in [1, 2] {
        group_entity(&mut sim, id, local);
        sim.entity_mut_for_test(id)
            .unwrap()
            .set_object_group_for_test(9);
    }
    apply_group_assignment(&mut sim, 0, &[]);
    assert!(sim.projected_control_groups()[0].is_empty());
}
