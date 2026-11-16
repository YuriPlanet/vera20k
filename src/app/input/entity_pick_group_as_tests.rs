//! Ares IDMatches732C30/1004A490, CollectSelectedIDs732D47/10041990,
//! click/band4ABD6C..4ABE58/10041760 and PlayerOwnedAlive7327AA/10041CD0.
use super::*;
use crate::{
    rules::ini_parser::IniFile,
    sim::{combat::Health, game_entity::GameEntity, world::Simulation},
};

fn rules(deploy: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[InfantryTypes]\n0=ALPHA\n1=COMMON\n\
         [VehicleTypes]\n0=BETA\n\
         [BuildingTypes]\n0=DEPLOYED\n\
         [General]\nTypeSelectUseDeploy={deploy}\n\
         [ALPHA]\nGroupAs=COMMON\nDeploysInto=DEPLOYED\n\
         [BETA]\nGroupAs=common\n\
         [DEPLOYED]\nUndeploysInto=BETA\n"
    )))
    .unwrap()
}

fn scene() -> Simulation {
    let mut sim = Simulation::new();
    let owner = sim.intern("me");
    let enemy = sim.intern("enemy");
    for (id, name, category) in [
        (1, "ALPHA", EntityCategory::Infantry),
        (2, "BETA", EntityCategory::Unit),
        (3, "COMMON", EntityCategory::Infantry),
        (4, "DEPLOYED", EntityCategory::Structure),
        (5, "ALPHA", EntityCategory::Infantry),
        (6, "BETA", EntityCategory::Unit),
    ] {
        let ty = sim.intern(name);
        let mut e = GameEntity::new_at_frame_zero_for_test(
            id,
            10,
            10,
            0,
            0,
            if id == 5 { enemy } else { owner },
            Health::new(100),
            ty,
            category,
            0,
            5,
            false,
        );
        e.lifecycle.set_object_alive_for_test(true);
        e.lifecycle.set_in_limbo_for_test(id == 6);
        sim.entities_mut_for_test().insert(e);
    }
    sim
}

/// Action 8's toggle needs the first selected House to be the player's
/// (0x004ABD35..0x004ABD57). A lone selected enemy makes the click action 7,
/// which unselects all (0x004ABE4A) before T's batch (0x004ABE67).
#[test]
fn shift_t_click_on_a_selected_enemy_replaces_it_with_the_players_type() {
    for ares_way in [false, true] {
        shift_t_click_replaces_a_selected_enemy(ares_way);
    }
}

fn shift_t_click_replaces_a_selected_enemy(ares_way: bool) {
    let _ares = crate::rules::ares_switch::read_ares_way(ares_way);
    let rules = rules("no");
    let ares = rules.ares();
    assert_eq!(ares, ares_way);
    let sim = scene();
    let click = compute_type_select_click_mutation_with_playfield(
        sim.entities(),
        &[1, 2, 3, 4, 5, 6],
        &[5],
        5,
        true,
        Some("me"),
        Some(&rules),
        Some(sim.interner()),
        false,
    );
    assert!(click.clear && click.deselect.is_empty());
    let grouped = if ares { vec![1, 2, 3] } else { vec![1] };
    assert_eq!(click.select, grouped);
}

#[test]
fn type_selection_group_reaches_tap_click_band_and_deploy_gate() {
    for ares_way in [false, true] {
        type_selection_group_reaches_every_path(ares_way);
    }
}

fn type_selection_group_reaches_every_path(ares_way: bool) {
    let _ares = crate::rules::ares_switch::read_ares_way(ares_way);
    let sim = scene();
    let order = [1, 2, 3, 4, 5, 6];
    let disabled_deploy = rules("no");
    let ares = disabled_deploy.ares();
    assert_eq!(ares, ares_way);
    let tap = |rules: &RuleSet| {
        compute_type_select_tap_with_playfield(
            sim.entities(),
            &order,
            &order,
            &[1],
            None,
            Some("me"),
            Some(rules),
            Some(sim.interner()),
            true,
            false,
        )
        .mutation
        .select
    };
    assert_eq!(
        tap(&disabled_deploy),
        if ares { vec![1, 2, 3] } else { vec![1, 4] }
    );
    assert_eq!(
        tap(&rules("yes")),
        if ares { vec![1, 2, 3, 4] } else { vec![1, 4] }
    );
    let grouped = if ares { vec![1, 2, 3] } else { vec![1] };
    let click = compute_type_select_click_mutation_with_playfield(
        sim.entities(),
        &order,
        &[],
        1,
        false,
        Some("me"),
        Some(&disabled_deploy),
        Some(sim.interner()),
        false,
    );
    assert!(click.clear);
    assert_eq!(click.select, grouped);
    let remove = compute_type_select_click_mutation_with_playfield(
        sim.entities(),
        &order,
        &[1, 2, 3, 4],
        1,
        true,
        Some("me"),
        Some(&disabled_deploy),
        Some(sim.interner()),
        false,
    );
    assert_eq!(remove.deselect, grouped);
    assert!(!remove.clear);
    #[derive(Clone, Copy)]
    struct AtPoint<'a>(&'a EntityStore);
    impl PickSource for AtPoint<'_> {
        fn entities(&self) -> &EntityStore {
            self.0
        }
        fn screen_position(&self, _: &GameEntity) -> (f32, f32) {
            (10.0, 10.0)
        }
    }
    let band = compute_type_select_box_mutation_with_playfield(
        AtPoint(sim.entities()),
        &[1, 2],
        &order,
        &[],
        None,
        Some("me"),
        0.0,
        0.0,
        20.0,
        20.0,
        false,
        Some(&disabled_deploy),
        Some(sim.interner()),
        false,
    );
    assert_eq!(band.select, if ares { vec![1, 2, 3] } else { vec![1, 2] });
}
