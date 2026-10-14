//! Rust regression tests for the reach block of `TechnoClass::AI_Update`
//! (`0x006FAAEF..0x006FABB8`, [`super::target_reach_step`]): a Foot outside a
//! team whose target lies in another zone runs Approach. Expected behaviour
//! comes from instruction reading; the search is Approach's own.

use super::{FrameEffects, RuleSet, Simulation, target_reach_step};
use crate::map::resolved_terrain::zone_class;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::{MissionId, MissionType};

/// A tank with a 4-cell gun: 896 leptons of Approach reach (`0x004D56F2`),
/// and a plain building.
fn rules() -> RuleSet {
    // This synthetic scene needs native buildable Clear ground and the
    // explicitly intended one-cell HUT foundation in the fixed ART reader.
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=TANK\n[BuildingTypes]\n0=HUT\n\
         [TANK]\nStrength=300\nArmor=heavy\nSpeed=6\nSight=10\nPrimary=TankGun\n\
         Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [HUT]\nStrength=100\n\
         [TankGun]\nDamage=10\nROF=20\nRange=4\nProjectile=InvisibleAll\nWarhead=HITWH\n\
         [InvisibleAll]\nInviso=yes\nAA=yes\nAG=yes\n\
         [HITWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str("[HUT]\nFoundation=1x1\n"))
        .expect("reach rules")
}

/// The flat arena, cut at x = 12..=13 by impassable cells when `split`, with
/// a human `Americans` `mover` at (`mover_x`, 10) on `mission` holding a
/// computer `Russians` `target` at (`target_x`, 10). Answers the mover and its
/// target.
fn hold(
    rules: &RuleSet,
    split: bool,
    (mover, mover_x): (&str, u16),
    (target, target_x): (&str, u16),
    mission: MissionType,
) -> (Simulation, u64, TargetKind) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x6FAAEF);
    if split {
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        for ry in 0..32 {
            for rx in 12..=13 {
                let cell = terrain.cell_mut(rx, ry).unwrap();
                cell.is_cliff_like = true;
                cell.ground_walk_blocked = true;
                cell.base_ground_walk_blocked = true;
                cell.zone_type = zone_class::IMPASSABLE;
            }
        }
        assert!(sim.rebuild_dynamic_navigation(rules));
    }
    let tank = sim
        .spawn_object_at_height(mover, "Americans", mover_x, 10, 0, 0, rules)
        .expect("mover spawns");
    let target = sim
        .spawn_object_at_height(target, "Russians", target_x, 10, 0, 0, rules)
        .expect("target spawns");
    let now = sim.session.binary_frame;
    sim.mission_assign_exact(tank, MissionId::from_known(mission), now)
        .unwrap();
    let _ = sim.mission_commence_exact(tank, now);
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(target)));
    (sim, tank, TargetKind::Entity(target))
}

/// [`hold`] with the tank at (8, 10) and a tank target.
fn pair(
    rules: &RuleSet,
    split: bool,
    target_x: u16,
    mission: MissionType,
) -> (Simulation, u64, TargetKind) {
    hold(rules, split, ("TANK", 8), ("TANK", target_x), mission)
}

fn reach(
    sim: &mut Simulation,
    rules: &RuleSet,
    tank: u64,
) -> (Option<TargetKind>, Option<NavTargetRef>) {
    target_reach_step(sim, tank, rules, None, FrameEffects::default());
    let tank = sim.substrate.entities.get(tank).unwrap();
    (tank.attack_target.target(), tank.navigation.nav_com)
}

/// A target in another zone is approached on any mission: from (8, 10) on
/// Guard the tank takes a cell on its own side within reach of the target
/// across the band at (14, 10), and keeps the target.
#[test]
fn a_guarding_tank_approaches_a_target_it_cannot_reach() {
    let rules = rules();
    let (mut sim, tank, held) = pair(&rules, true, 14, MissionType::Guard);

    let (target, nav_com) = reach(&mut sim, &rules, tank);

    assert_eq!(target, Some(held));
    let Some(NavTargetRef::Cell { rx, ry }) = nav_com else {
        panic!("Approach sets a firing cell: {nav_com:?}");
    };
    assert!(
        rx < 12,
        "the firing cell ({rx}, {ry}) lies on the tank's side"
    );
    let (dx, dy) = (i32::from(rx) - 14, i32::from(ry) - 10);
    assert!(
        (dx * dx + dy * dy) * 256 * 256 <= 896 * 896,
        "({rx}, {ry}) lies within reach"
    );
}

/// A reachable target ends the block (`0x006FAB52`): out of range on Guard,
/// the tank neither approaches it nor lets it go; Guard has no Approach.
#[test]
fn a_tank_leaves_a_reachable_target_to_its_mission() {
    let rules = rules();
    let (mut sim, tank, held) = pair(&rules, false, 16, MissionType::Guard);

    assert_eq!(reach(&mut sim, &rules, tank), (Some(held), None));
}

/// A team member (vt+0x290, `0x0041C050`) skips the block whatever its
/// target.
#[test]
fn a_team_member_leaves_an_unreachable_target_to_its_team() {
    let rules = rules();
    let (mut sim, tank, held) = pair(&rules, true, 14, MissionType::Guard);
    crate::sim::team_script_vm::join_team_for_test(&mut sim, &[tank], false, false);

    assert_eq!(reach(&mut sim, &rules, tank), (Some(held), None));
}

/// On Sticky, an out-of-range target is dropped by Approach
/// (`0x004D5716..0x004D5744`), which the block runs only for a target in
/// another zone: a reachable one is kept.
#[test]
fn a_sticky_tank_drops_only_a_target_it_cannot_reach() {
    let rules = rules();
    for (split, dropped) in [(true, true), (false, false)] {
        let (mut sim, tank, held) = pair(&rules, split, 14, MissionType::Sticky);

        let kept = (!dropped).then_some(held);
        assert_eq!(
            reach(&mut sim, &rules, tank),
            (kept, None),
            "split: {split}"
        );
    }
}

/// A building's vt+0x4C for a mover is its centre (`0x00447E90` returns +0x48
/// unless the type is a helipad, repair bay or bunker), and structures leave
/// the terrain zones as they are: a building in the tank's zone is reachable,
/// so Guard keeps it without approaching.
#[test]
fn a_building_in_the_tanks_zone_is_reachable() {
    let rules = rules();
    let (mut sim, tank, held) = hold(&rules, false, ("TANK", 8), ("HUT", 16), MissionType::Guard);

    assert_eq!(reach(&mut sim, &rules, tank), (Some(held), None));
}

/// When Approach leaves no NavCom, the block's own CanFireAt decides
/// (`0x006FAB7A..0x006FABB2`). A Magnetron holding its target (`+0x2AC`)
/// returns from Approach at once (`0x004D56A1..0x004D56B1`), so a target
/// across the band and out of range is dropped by the block alone.
#[test]
fn the_block_drops_a_target_approach_leaves_out_of_range() {
    let rules = rules();
    let (mut sim, tank, held) = pair(&rules, true, 14, MissionType::Guard);
    let TargetKind::Entity(target) = held else {
        unreachable!()
    };
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .locomotor_target = Some(target);

    assert_eq!(reach(&mut sim, &rules, tank), (None, None));
}

/// A target in another zone but in range: Approach returns before its search
/// (`0x004D5A34`) and CanFireAt keeps it, with no destination.
#[test]
fn an_unreachable_target_in_range_is_kept_without_a_destination() {
    let rules = rules();
    let (mut sim, tank, held) = hold(&rules, true, ("TANK", 11), ("TANK", 14), MissionType::Guard);

    assert_eq!(reach(&mut sim, &rules, tank), (Some(held), None));
}
