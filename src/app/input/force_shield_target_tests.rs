//! The manual pointer gate is distinct from stock Super::Launch admission.
//! Original Type6CEF80 / House4F9A50 and the release suffix4AC20C are executed
//! by tools.sidebar_oracle.superweapon_click. Actual mouse integration is
//! exercised separately through the normal map-observation capture.

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::{
    combat::Health, game_entity::GameEntity, house_state::HouseState, world::Simulation,
};

fn pointer_scene(input: &serde_json::Value) -> (Simulation, InternedId, Option<u64>) {
    let mut sim = Simulation::new();
    let viewer = sim.intern("Viewer");
    let other = sim.intern("Other");
    for (index, owner) in [viewer, other].into_iter().enumerate() {
        sim.houses_mut_for_test().register(HouseState::new(
            owner,
            index as u8,
            None,
            index == 0,
            1000,
            10,
        ));
    }
    for (key, asker, target, bit) in [
        ("local_allies", "VIEWER", "OTHER", 2),
        ("target_allies", "OTHER", "VIEWER", 1),
    ] {
        if input[key].as_u64().unwrap() & bit != 0 {
            // Supplied native House-mask input; production diplomacy remains
            // the owner of live changes. No alliance state is added here.
            sim.house_alliances_mut_for_test()
                .entry(asker.into())
                .or_default()
                .insert(target.into());
        }
    }
    let picked = input["target_kind"].as_i64().map(|kind| {
        let type_id = sim.intern("PointerTarget");
        let owner = if input["target_owner"] == 0 {
            viewer
        } else {
            other
        };
        let category = match kind {
            6 => EntityCategory::Structure,
            28 => EntityCategory::Unit,
            _ => panic!("unexpected native target class {kind}"),
        };
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            10,
            0,
            0,
            owner,
            Health::new(100),
            type_id,
            category,
            0,
            5,
            false,
        );
        entity.lifecycle.set_object_alive_for_test(true);
        entity.lifecycle.set_in_limbo_for_test(false);
        sim.entities_mut_for_test().insert(entity);
        1
    });
    (sim, viewer, picked)
}

#[test]
fn pointer_admission_matches_original_action_and_release() {
    let corpus = crate::test_fixture::json("tools/sidebar_oracle/superweapon_click.json");
    let rows = corpus["pointer_targets"].as_array().unwrap();
    assert_eq!(rows.len(), 11);
    for row in rows {
        let input = &row["input"];
        let (sim, viewer, picked) = pointer_scene(input);
        let kind = match input["super_kind"].as_i64().unwrap() {
            0 => SuperWeaponKind::MultiMissile,
            1 => SuperWeaponKind::IronCurtain,
            10 => SuperWeaponKind::ForceShield,
            other => panic!("unexpected native Super kind {other}"),
        };
        let before = sim.state_hash();
        let accepted = super_weapon_pointer_target_is_valid(&sim, viewer, kind, picked);
        assert_eq!(
            accepted,
            row["expected"]["event_count"] == 1,
            "{}: original release event",
            input["name"]
        );
        assert_eq!(
            accepted,
            row["expected"]["action"] == input["configured_action"],
            "{}: original type action",
            input["name"]
        );
        assert_eq!(sim.state_hash(), before, "pointer queries are read-only");
    }
}

#[test]
fn pointer_admission_rechecks_directional_alliance_after_arming() {
    let (mut sim, viewer, picked) = pointer_scene(&serde_json::json!({
        "target_kind": 6, "target_owner": 1, "local_allies": 0, "target_allies": 0
    }));
    let admitted = |sim: &Simulation| {
        super_weapon_pointer_target_is_valid(sim, viewer, SuperWeaponKind::ForceShield, picked)
    };
    assert!(!admitted(&sim));
    sim.house_alliances_mut_for_test()
        .entry("VIEWER".into())
        .or_default()
        .insert("OTHER".into());
    assert!(
        !admitted(&sim),
        "the player's view of the target is not the native asker"
    );
    sim.house_alliances_mut_for_test()
        .entry("OTHER".into())
        .or_default()
        .insert("VIEWER".into());
    assert!(admitted(&sim));
    sim.house_alliances_mut_for_test()
        .get_mut("OTHER")
        .unwrap()
        .remove("VIEWER");
    assert!(
        !admitted(&sim),
        "an earlier preview must not retain an alliance"
    );
}

#[test]
fn stock_command_target_check_remains_separate_from_pointer_gate() {
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[SuperWeaponTypes]\n0=Shield\n[Shield]\nType=ForceShield\nAction=ForceShield\n",
    );
    let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
    let mut sim = Simulation::new();
    let viewer = sim.intern("Viewer");
    let shield = sim.intern("Shield");
    assert!(!super_weapon_pointer_target_is_valid(
        &sim,
        viewer,
        SuperWeaponKind::ForceShield,
        None
    ));
    // Type6CEF80 belongs to DetermineAction, not ClickFire/Launch. AI,
    // trigger and immediate Action=None cameo producers have no pointer.
    assert_eq!(
        sim.super_weapon_target_check(&rules, viewer, shield, (12, 12), false),
        Ok(())
    );
    assert_eq!(
        sim.super_weapon_target_check(&rules, viewer, shield, (12, 12), true),
        Ok(())
    );
    assert_eq!(
        super_weapon_click_command(&sim, &rules, viewer, shield, (12, 12)),
        Some(Command::LaunchSuperWeapon {
            sw_type_id: shield,
            target_rx: 12,
            target_ry: 12,
        })
    );
}
