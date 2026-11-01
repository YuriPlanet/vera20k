//! Infantry51F838..51F85B rewrites an Attack Cell action for negative average
//! weapon damage. The map override activates it with an existing retail weapon.

use super::*;
use crate::app::input::cursor::{ActionQuery, cursor_id_for_feedback};
use crate::app::types::{CursorFeedbackKind, CursorId};
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::Simulation;

#[test]
fn retail_healing_infantry_ctrl_cell_guards_the_clicked_post() {
    let patch = IniFile::from_str("[E1]\nPrimary=RepairBullet\n");
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "Hills.mmx",
        &patch,
    ) else {
        return;
    };
    let overlays = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let (mut sim, rules, actor) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
    assert_eq!(
        crate::sim::combat::combat_weapon::weapon_damage_value(
            sim.entities().get(actor).unwrap(),
            rules.object("E1").unwrap(),
            sim.entities(),
            &rules,
            sim.interner(),
        ),
        -12,
        "RepairBullet -50 and the retained Para 25 use the native signed average"
    );
    let selected = [actor];
    let query = ActionQuery {
        sim: &sim,
        rules: Some(&rules),
        overlays: Some(&overlays),
        viewer: sim.session().current_house(),
        selected: &selected,
        modifier: OrderModifier::ForceFire,
        force_move_key: false,
        order_mode: OrderMode::AttackMove,
        planning: false,
        mouse_over: true,
        full_visibility: true,
    };
    let before = sim.state_hash();
    let feedback = query.selection_action((5, 6), None);
    let click = attack_branch_orders(
        &sim,
        &rules,
        &selected,
        TargetKind::Cell(5, 6),
        true,
        OrderMode::AttackMove,
        &overlays,
    );
    let expected = Command::Guard {
        entity_id: actor,
        target: Some(TargetKind::Cell(5, 6)),
    };
    assert_eq!(
        (feedback, click.commands.as_slice()),
        (
            Some(CursorFeedbackKind::GuardArea),
            std::slice::from_ref(&expected)
        )
    );
    assert_eq!(
        cursor_id_for_feedback(feedback.unwrap()),
        Some(CursorId::GuardArea)
    );
    assert!(click.consumed && !click.select_fallback);
    assert_eq!(
        sim.state_hash(),
        before,
        "input queries must not change gameplay or RNG"
    );
    assert!(sim.apply_command("Local", &click.commands[0], Some(&rules)));
    let actor = sim.entities().get(actor).unwrap();
    assert_eq!(
        actor.mission.queued(),
        MissionId::from_known(MissionType::AreaGuard)
    );
    assert_eq!(actor.archive_target(), Some(TargetKind::Cell(5, 6)));
    assert_eq!(
        actor.navigation.nav_com(),
        Some(TargetKind::Cell(5, 6).into())
    );
    assert!(actor.attack_target.target().is_none());
}

#[test]
fn retail_damaging_infantry_ctrl_cell_still_attacks() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let overlays = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let (sim, rules, actor) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
    assert_eq!(
        attack_branch_orders(
            &sim,
            &rules,
            &[actor],
            TargetKind::Cell(5, 6),
            true,
            OrderMode::Move,
            &overlays,
        )
        .commands,
        [Command::ForceAttackCell {
            attacker_id: actor,
            target_rx: 5,
            target_ry: 6
        }]
    );
}
