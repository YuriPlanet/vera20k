//! VoxelAnim749F30's meteor and Tiberium expiry with retail [VoxelAnims]
//! types on flat formed terrain. Each object flies its ordinary AI visits.

use crate::map::waypoints::Waypoint;
use crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::meteor_tests::formed_sim;
use crate::sim::world::{FrameEffects, Simulation};
use glam::IVec3;
use std::collections::HashMap;

/// Visits one VoxelAnim each frame until its UnInit drains and returns the
/// Bounce coords its expiry read.
pub(in crate::sim) fn fly_to_expiry(sim: &mut Simulation, rules: &RuleSet, id: u64) -> IVec3 {
    let mut impact = sim
        .voxel_anims()
        .get(id)
        .unwrap()
        .bounce()
        .position_leptons();
    for _ in 0..1000 {
        sim.visit_voxel_anim(id, rules, FrameEffects::default());
        sim.process_pending_delete_with(
            rules,
            Some(rules.overlay_registry()),
            FrameEffects::default(),
            1,
            sim.session().started(),
        );
        let Some(piece) = sim.voxel_anims().get(id) else {
            return impact;
        };
        impact = piece.bounce().position_leptons();
    }
    panic!("voxel anim {id} never expired");
}

/// Waypoint 7 on cell 40,40 of a flat level-0 map, with the rule handles
/// scenario init resolves before any area damage.
pub(in crate::sim) fn waypoint_sim(rules: &RuleSet) -> Simulation {
    let mut sim = formed_sim(31, 0);
    sim.resolve_type_handles(rules);
    sim.session.install_waypoints(&HashMap::from([(
        7,
        Waypoint {
            index: 7,
            rx: 40,
            ry: 40,
        },
    )]));
    sim
}

/// The overlay name and density on one cell.
fn overlay(sim: &Simulation, rules: &RuleSet, x: i32, y: i32) -> (Option<String>, u8) {
    let cell = sim.overlay_grid.as_ref().unwrap().cell(x as u16, y as u16);
    let name = cell
        .overlay_id
        .and_then(|id| rules.overlay_registry().name(id))
        .map(str::to_owned);
    (name, cell.overlay_data)
}

/// A flat Vinifera image at density 0: TIB2_01..TIB2_12.
fn assert_new_vinifera(sim: &Simulation, rules: &RuleSet, x: i32, y: i32) {
    let (name, density) = overlay(sim, rules, x, y);
    let image = name
        .as_deref()
        .and_then(|name| name.strip_prefix("TIB2_"))
        .and_then(|image| image.parse::<u8>().ok());
    assert!(
        image.is_some_and(|image| (1..=12).contains(&image)),
        "{name:?} at {x},{y}"
    );
    assert_eq!(density, 0, "at {x},{y}");
}

/// Meteor expiry 74A303..572 seeds the eight neighbours of METEOR02's
/// impact cell, never the cell itself; ordinary expiry 74A5B6..6F7 seeds
/// PEBBLE's own cell.
#[test]
fn retail_meteor02_rings_its_impact_cell_with_ore_and_pebble_seeds_its_own() {
    let Some(rules) = retail_unlayered_rules_with_art() else {
        return;
    };
    let meteor = rules.voxel_anim_type_id_by_name("METEOR02").unwrap();
    let mut sim = waypoint_sim(&rules);
    assert!(sim.trigger_play_voxel_anim_at_waypoint(&rules, meteor.0 as i32, 7));
    let id = sim.voxel_anims().ids()[0];
    let impact = fly_to_expiry(&mut sim, &rules, id);
    let (x, y) = (impact.x / 256, impact.y / 256);
    assert_eq!(overlay(&sim, &rules, x, y), (None, 0));
    for (dx, dy) in crate::sim::tiberium::ADJACENT_OFFSETS {
        assert_new_vinifera(&sim, &rules, x + dx, y + dy);
    }

    let pebble = rules.voxel_anim_type_id_by_name("PEBBLE").unwrap();
    let mut sim = waypoint_sim(&rules);
    sim.emit_debris_voxel(
        pebble,
        rules.voxel_anim_type(pebble),
        None,
        IVec3::new(64 * 256 + 128, 64 * 256 + 128, 0),
        true,
        &mut Vec::new(),
    )
    .unwrap();
    let id = sim.voxel_anims().ids()[0];
    let impact = fly_to_expiry(&mut sim, &rules, id);
    assert_new_vinifera(&sim, &rules, impact.x / 256, impact.y / 256);
}

/// Meteor expiry 74A035..099 on Water below the deck: only the last
/// SplashList entry at Location+5. No ExpireAnim, children or ore follow.
#[test]
fn retail_meteor_water_landing_ends_at_the_last_splash() {
    let Some(mut rules) = retail_unlayered_rules_with_art() else {
        return;
    };
    let splash = rules.combat_damage.splash_list.last().unwrap().clone();
    rules.bind_anim_frame_count_for_test(&splash, 16);
    let meteor = rules.voxel_anim_type_id_by_name("METEOR02").unwrap();
    let mut sim = waypoint_sim(&rules);
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(40, 40)
        .unwrap()
        .land_type = crate::rules::terrain_rules::LandType::Water.as_index();
    assert!(sim.trigger_play_voxel_anim_at_waypoint(&rules, meteor.0 as i32, 7));
    let id = sim.voxel_anims().ids()[0];
    let impact = fly_to_expiry(&mut sim, &rules, id);
    assert_eq!((impact.x / 256, impact.y / 256), (40, 40));
    assert!(sim.voxel_anims().ids().is_empty(), "no children");
    let anims: Vec<_> = sim.anims().map(|(_, anim)| anim).collect();
    assert_eq!(anims.len(), 1);
    assert_eq!(sim.interner.resolve(anims[0].type_id()), splash);
    assert_eq!(anims[0].draw_flags(), 0x600);
    let coord = anims[0].world_coord();
    assert_eq!(
        [coord.x, coord.y, coord.z],
        [impact.x, impact.y, impact.z + 5]
    );
    for (dx, dy) in crate::sim::tiberium::ADJACENT_OFFSETS {
        assert_eq!(overlay(&sim, &rules, 40 + dx, 40 + dy), (None, 0));
    }
}
