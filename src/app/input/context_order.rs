//! Context-sensitive order resolution — translates a screen click into game commands.
//!
//! Given a click position and the current selection, determines what command to issue:
//! move, attack, garrison, deploy, harvest, rally point, etc. This is the decision tree
//! that maps player intent to `Command` envelopes.
//!
//! Split from the input dispatcher to separate order resolution from raw input handling.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

#[cfg(test)]
use crate::app::input::entity_pick::HoverObject;
#[cfg(test)]
use crate::app::input::entity_pick::HoverTargetKindWithId;
#[cfg(test)]
use crate::app::types::HoverTargetKind;
#[cfg(test)]
use crate::app::types::OrderMode;
#[cfg(test)]
use crate::map::entities::EntityCategory;
#[cfg(test)]
use crate::sim::combat::TargetKind;
#[cfg(test)]
use crate::sim::command::Command;
#[cfg(test)]
use crate::sim::command::CommandEnvelope;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::world::ControlProjection;

/// Preserve the native owner test for stock self-clicks. A custom seat
/// projects the same actor permission as its eventual command.
fn context_owns_actor(
    sim: &crate::sim::world::Simulation,
    actor: &GameEntity,
    local_owner: InternedId,
) -> bool {
    sim.control_projection()
        .can_order_from(actor.owner(), local_owner)
}

fn context_order_receiver(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    actor: u64,
) -> bool {
    !sim.session().has_custom_control_grants() || projected_actor_can_order(sim, control, actor)
}

pub(super) fn projected_actor_can_order(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    actor: u64,
) -> bool {
    sim.entities()
        .get(actor)
        .is_some_and(|entity| control.can_order(entity.owner()))
}

/// Use one ordered receiver list for tactical and radar previews and release.
/// A selected carrier may still detonate a bomb owned by a controllable House.
pub(super) fn projected_order_selection<'a>(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    selected: &'a [u64],
    target: Option<u64>,
) -> std::borrow::Cow<'a, [u64]> {
    use std::borrow::Cow;
    if !sim.session().has_custom_control_grants()
        || rules
            .zip(target)
            .is_some_and(|(rules, target)| self_detonates(sim, rules, selected, target))
    {
        return Cow::Borrowed(selected);
    }

    let control = sim.control_projection();
    let can_order = |actor| projected_actor_can_order(sim, &control, actor);
    let Some(first_denied) = selected.iter().position(|&actor| !can_order(actor)) else {
        return Cow::Borrowed(selected);
    };
    let mut receivers = selected[..first_denied].to_vec();
    receivers.extend(
        selected[first_denied + 1..]
            .iter()
            .copied()
            .filter(|&actor| can_order(actor)),
    );
    Cow::Owned(receivers)
}

fn context_actor_viewer(
    sim: &crate::sim::world::Simulation,
    actor_owner: InternedId,
) -> Option<InternedId> {
    if sim.session().has_custom_control_grants() {
        sim.session().current_house()
    } else {
        Some(actor_owner)
    }
}

mod action;
mod building;
mod click_command;
mod entry;
mod formation;
mod infantry;
mod orders;
mod queue;
mod self_click;

pub(crate) use action::AttackAction;
use action::AttackDispatch;
pub(super) use action::HealerAction;
pub(crate) use action::OrderModifier;
pub(crate) use action::armed_action;
use action::armed_action_with;
pub(crate) use action::attack_action;
use action::attack_action_with;
pub(super) use action::ctrl_forces_attack_rung;
pub(super) use action::healer_guards_itself;
pub(super) use action::healer_supports_over_terrain;
pub(super) use action::healer_object_action;
use action::healer_object_action_with;
pub(super) use action::infantry_arm_attacks;
use action::infantry_arm_attacks_with;
pub(super) use action::object_action_answers_friendly;
#[cfg(test)]
use action::object_attack_is_admitted;
pub(crate) use action::resolve_order_modifiers;
pub(crate) use action::resolved_unit_in_range;
use action::shrouded_object_action;
use action::target_is_immune;
use building::BuildingClick;
pub(super) use building::ares_trench_enter;
#[cfg(test)]
pub(super) use building::building_cell_orders;
use building::building_click_orders;
use building::building_selection_click;
#[cfg(test)]
use building::selected_rally_producer_ids;
pub(super) use click_command::ObjectKeyAction;
use click_command::attack_support_command;
use click_command::healer_terrain_key_order;
pub(super) use click_command::cell_guard_area_action;
use click_command::cell_guard_area_action_with;
use click_command::cell_harvest_order;
use click_command::entity_can_attack_move;
use click_command::guard_area_admitted;
use click_command::move_click_command;
#[cfg(test)]
use click_command::object_attack_click_command;
use click_command::object_attack_click_command_with;
use click_command::object_click_payload;
use click_command::object_guard_command;
pub(super) use click_command::object_key_action;
use click_command::object_key_action_with;
use click_command::object_key_target;
use click_command::push_move_flash;
pub(super) use click_command::selection_can_attack_move;
pub(super) use click_command::terrain_forced;
use entry::engineer_capture_orders;
#[cfg(test)]
use entry::entry_action;
use entry::entry_orders;
use entry::garrison_entry_orders;
pub(super) use entry::hovered_entry_action;
pub(super) use entry::infantry_garrisons;
use entry::infantry_garrisons_with;
pub(super) use entry::vehicle_capture_action;
pub(crate) use formation::formation_unit_count;
use formation::locked_move_cells;
pub(crate) use formation::queue_formation_line;
pub(crate) use formation::toggle_formation_lock;
pub(super) use infantry::InfantryAttackAction;
pub(super) use infantry::InfantryC4Action;
pub(super) use infantry::ares_ivan_grinds;
use infantry::cursor_owner;
use infantry::disarm_bomb_engineers;
pub(super) use infantry::infantry_attack_action;
use infantry::infantry_attack_action_with;
pub(super) use infantry::infantry_c4_action;
use infantry::infantry_c4_action_with;
use infantry::infiltrator_capture_orders;
pub(crate) use infantry::ivan_bomb_object_refused;
pub(crate) use infantry::ivan_bomb_refused;
use infantry::ivan_cannot_bomb;
use infantry::object_click_release;
use infantry::shot_is_illegal;
use infantry::{FriendlyIvanAction, friendly_ivans};
use orders::attack_branch_orders;
#[cfg(test)]
use orders::command_actor_id;
use orders::finish_order;
#[cfg(test)]
use orders::order_voice_key;
#[cfg(test)]
use orders::restore_selection_dispatch_order;
#[cfg(test)]
use orders::retain_context_orders;
use orders::unit_repair_branch_orders;
pub(crate) use queue::ContextOrderTarget;
use queue::LeftRelease;
use queue::ReleaseAction;
#[cfg(test)]
use queue::berserk_selection_click;
#[cfg(test)]
use queue::{HeadRelease, head_release};
use queue::left_release;
pub(crate) use queue::{ContextClick, try_queue_context_order};
pub(super) use self_click::AresDeactivatedAction;
pub(super) use self_click::ares_deactivated_action;
pub(super) use self_click::ares_emp_deployer_refuses_itself;
use self_click::ares_head_hook_selection_click;
pub(super) use self_click::is_simple_deploy_unit;
use self_click::lone_self_foot_release;
use self_click::lone_self_healer_guard;
pub(super) use self_click::self_answers_no_deploy;
pub(super) use self_click::self_click_command;
pub(super) use self_click::self_detonates;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "building_redeploy_tests.rs"]
mod building_redeploy_tests;

#[cfg(all(test, tests_app_input_a_c))]
mod tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "context_order_selection_tests.rs"]
mod selection_dispatch_tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "context_order_move_flash_tests.rs"]
mod move_flash_tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "context_order_retail_bridge_tests.rs"]
mod retail_bridge_tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "context_order_retail_engineer_tests.rs"]
mod retail_engineer_tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "context_order_retail_garrison_tests.rs"]
mod retail_garrison_tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "context_order_retail_entry_tests.rs"]
mod retail_entry_tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "disguise_cursor_tests.rs"]
mod disguise_cursor_tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "context_order_retail_terrain_tests.rs"]
mod retail_terrain_tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "context_order_retail_inactive_unit_tests.rs"]
mod retail_inactive_unit_tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "context_order_retail_self_deploy_tests.rs"]
mod retail_self_deploy_tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "building_bunker_self_click_tests.rs"]
mod building_bunker_self_click_tests;

#[cfg(all(
    test,
    any(
        tests_app_input_a_c,
        tests_app_presentation_instances,
        tests_app_presentation_render
    )
))]
#[path = "context_order_retail_ifv_repair_tests.rs"]
pub(in crate::app) mod retail_ifv_repair_tests;

#[cfg(all(test, tests_app_input_a_c))]
mod stationary_attack_dispatch_tests {
    use super::*;
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::docking::bunker_link::BunkerLink;
    use crate::sim::world::Simulation;

    #[test]
    fn bunker_attack_batch_preserves_mobile_order_and_select_fallback() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            r#"
[VehicleTypes]
0=TANK
[TANK]
Strength=300
Primary=GUN
Speed=6
SpeedType=Track
MovementZone=Normal
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
[GUN]
Damage=60
Range=5
Projectile=SHOT
Warhead=AP
[SHOT]
AG=yes
Inviso=yes
SubjectToElevation=no
[Warheads]
0=AP
[AP]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
"#,
        ))
        .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let stationary = sim
            .spawn_object("TANK", "Americans", 2, 2, 0, &rules)
            .unwrap();
        let mobile = sim
            .spawn_object("TANK", "Americans", 2, 4, 0, &rules)
            .unwrap();
        let friend = sim
            .spawn_object("TANK", "Americans", 20, 2, 0, &rules)
            .unwrap();
        let enemy = sim
            .spawn_object("TANK", "Soviets", 20, 4, 0, &rules)
            .unwrap();
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let house = Some(sim.entities().get(stationary).unwrap().owner());
        sim.session_mut_for_test().set_current_house_for_test(house);
        sim.entity_mut_for_test(stationary)
            .unwrap()
            .bunker
            .set_bunker_link_for_test(BunkerLink::Installed(99));
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
            32,
            32,
            (0..32)
                .flat_map(|y| {
                    (0..32).map(move |x| crate::map::resolved_terrain::test_flat_cell(x, y))
                })
                .collect(),
        ));
        let registry = OverlayTypeRegistry::empty();
        let mixed = attack_branch_orders(
            &sim,
            &rules,
            &[stationary, mobile],
            TargetKind::Cell(20, 2),
            true,
            OrderMode::Move,
            &registry,
        );
        assert!(mixed.consumed);
        assert!(!mixed.select_fallback);
        assert_eq!(mixed.receivers, [mobile].into_iter().collect());
        assert_eq!(
            crate::app::input::commands::ordinary_cell_move_goal(
                &sim,
                &rules,
                house.unwrap(),
                stationary,
                (20, 2),
                false,
            ),
            None,
            "the remaining stationary receiver cannot commit a Cell move"
        );
        assert_eq!(mixed.commands.len(), 1);
        assert!(
            matches!(mixed.commands[0], Command::ForceAttackCell { attacker_id, .. } if attacker_id==mobile)
        );
        let none = attack_branch_orders(
            &sim,
            &rules,
            &[stationary],
            TargetKind::Entity(enemy),
            true,
            OrderMode::Move,
            &registry,
        );
        assert!(none.commands.is_empty());
        assert!(
            none.consumed && !none.select_fallback,
            "Action0 consumes without selection or a hidden attack event"
        );
        let select = attack_branch_orders(
            &sim,
            &rules,
            &[stationary],
            TargetKind::Entity(friend),
            true,
            OrderMode::Move,
            &registry,
        );
        assert!(select.commands.is_empty());
        assert!(
            !select.consumed && select.select_fallback,
            "Action7 leaves the click for ordinary selection"
        );
        let near = attack_branch_orders(
            &sim,
            &rules,
            &[stationary],
            TargetKind::Cell(4, 2),
            true,
            OrderMode::Move,
            &registry,
        );
        assert_eq!(near.commands.len(), 1);
        assert!(
            matches!(near.commands[0], Command::ForceAttackCell { attacker_id, .. } if attacker_id==stationary)
        );
    }
}

#[cfg(all(test, tests_app_input_a_c))]
#[path = "allied_object_action_tests.rs"]
mod allied_object_action_tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "context_order_cursor_contract_tests.rs"]
mod cursor_contract_tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "engineer_special_entry_tests.rs"]
mod engineer_special_entry_tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "foot_unarmed_ctrl_tests.rs"]
mod foot_unarmed_ctrl_tests;

#[cfg(all(test, tests_app_input_a_c))]
#[path = "infantry_healer_cell_tests.rs"]
mod infantry_healer_cell_tests;
