use super::*;
use crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art;
use crate::sim::anim_class::meteor_tests::{formed_rules, formed_sim};
use crate::sim::world::voxel_meteor_tests::{fly_to_expiry, waypoint_sim};
#[test]
fn counted_action58_uses_literal_intensity_and_tail_waypoint_through_live_dispatch() {
    let rules = formed_rules("");
    let actions = crate::map::actions::parse_actions(&crate::rules::ini_parser::IniFile::from_str(
        "[Actions]\nCUE=1,58,0,0,0,0,0,0,H\n",
    ));
    let entry = actions["CUE"].entries()[0].clone();
    assert_eq!(entry.literal_value(), Some(0));
    assert_eq!(entry.waypoint_index, Some(7));
    assert_eq!(
        crate::map::actions::trigger_anim_class_roots(&rules, &actions),
        ["METLARGE", "METSMALL"]
    );
    let mut sim = formed_sim(31, 2);
    sim.session
        .install_waypoints(&std::collections::HashMap::from([(
            7,
            crate::map::waypoints::Waypoint {
                index: 7,
                rx: 40,
                ry: 40,
            },
        )]));
    let mut direct = formed_sim(31, 2);
    direct
        .session
        .install_waypoints(&std::collections::HashMap::from([(
            7,
            crate::map::waypoints::Waypoint {
                index: 7,
                rx: 40,
                ry: 40,
            },
        )]));
    let action = RuntimeDefinitions::default().resolve_action(entry, None, None, None);
    let mut effects = Vec::new();
    assert!(sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            incoming_house: None,
            input: TagEventInput::default()
        },
        &mut effects,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::empty()
    ));
    assert!(direct.trigger_meteor_shower(&rules, 0, 7));
    assert_eq!(sim.state_hash(), direct.state_hash());
    assert!(effects.is_empty());
    assert_eq!(sim.anims().count(), direct.anims().count());
    //A same-named ART body cannot allocate the missing native type.
    let mut missing = formed_rules("");
    missing.anim_type_names.clear();
    assert!(crate::map::actions::trigger_anim_class_roots(&missing, &actions).is_empty());
    let mut invalid = formed_sim(31, 2);
    invalid
        .session
        .install_waypoints(&std::collections::HashMap::from([(
            7,
            crate::map::waypoints::Waypoint {
                index: 7,
                rx: 40,
                ry: 40,
            },
        )]));
    assert!(!invalid.trigger_meteor_shower(&missing, 0, 7));
    assert_eq!(invalid.anims().count(), 0);
}

/// Active43 PlayVoxelAnimAtWP6E2520 through live dispatch: retail
/// METEOR01 starts above its waypoint (749668..74981F), keeps a straight
/// line back to the waypoint cell (74A7C8) and scatters PEBBLEs where it
/// ends (74A288..301).
#[test]
fn action43_drops_retail_meteor01_on_its_waypoint_cell_and_scatters_pebbles() {
    let Some(rules) = retail_unlayered_rules_with_art() else {
        return;
    };
    let meteor = rules.voxel_anim_type_id_by_name("METEOR01").unwrap();
    let pebble = rules.voxel_anim_type_id_by_name("PEBBLE").unwrap();
    let actions = crate::map::actions::parse_actions(&crate::rules::ini_parser::IniFile::from_str(
        &format!("[Actions]\nCUE=1,43,0,{},0,0,0,0,H\n", meteor.0),
    ));
    let entry = actions["CUE"].entries()[0].clone();
    let action = RuntimeDefinitions::default().resolve_action(entry, None, None, None);
    let mut sim = waypoint_sim(&rules);
    assert!(sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            incoming_house: None,
            input: TagEventInput::default()
        },
        &mut Vec::new(),
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::empty()
    ));
    let ids = sim.voxel_anims().ids();
    assert_eq!(ids.len(), 1);
    let piece = sim.voxel_anims().get(ids[0]).unwrap();
    assert_eq!(piece.type_id(), meteor);
    assert_eq!(piece.owner_house(), None);
    assert!(piece.in_logic_vector());
    // Unlimbo749794 and Bounce4397E0 both take the raised launch start.
    assert_eq!(piece.world_coord(), piece.bounce().position_leptons());
    assert!(piece.world_coord().z > 0);
    let impact = fly_to_expiry(&mut sim, &rules, ids[0]);
    assert_eq!((impact.x / 256, impact.y / 256), (40, 40));
    let children = sim.voxel_anims().ids();
    assert!(!children.is_empty());
    assert!(children.len() as i32 <= 2 * rules.voxel_anim_type(meteor).spawn_count);
    for child in children {
        let child = sim.voxel_anims().get(child).unwrap();
        assert_eq!(child.type_id(), pebble);
        assert_eq!(child.owner_house(), None);
        assert_eq!(child.world_coord(), impact);
    }
}
