//! Laser fences through their production owners: a post's Unlimbo raising
//! sections to the next post, their frames on and off with the posts'
//! power, the Foot burn, passability, the post's removal and the scenario
//! pass.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::world::FrameEffects;

const RULES: &str = "[Countries]\n0=Americans\n1=Russians\n\
     [BuildingTypes]\n0=POST\n1=FENCE\n\
     [POST]\nStrength=600\nLaserFencePost=yes\nGuardRange=8\nSight=0\n\
     [FENCE]\nStrength=256000\nLaserFence=yes\nSight=0\n\
     [InfantryTypes]\n0=E1\n[E1]\nStrength=125\nSight=0\n\
     [Warheads]\n0=C4WH\n[C4WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
     [CombatDamage]\nC4Warhead=C4WH\n";

fn arena() -> (Simulation, RuleSet) {
    let art = IniFile::from_str("[POST]\nFoundation=1x1\n[FENCE]\nFoundation=1x1\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &art,
    )
    .unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x00452DC0);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    (sim, rules)
}

/// The cell's first building and its frame.
fn section(sim: &Simulation, x: u16, y: u16) -> Option<(u64, i32)> {
    let id = sim
        .substrate
        .occupancy
        .first_building_on_layer(x, y, MovementLayer::Ground)?;
    Some((id, sim.substrate.entities.get(id)?.laser_fence_frame.frame()))
}

fn frames(sim: &Simulation, cells: &[(u16, u16)]) -> Vec<Option<i32>> {
    cells
        .iter()
        .map(|&(x, y)| section(sim, x, y).map(|(_, frame)| frame))
        .collect()
}

fn alive(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.is_object_alive() && entity.health.current() > 0)
}

const RUN: [(u16, u16); 4] = [(10, 14), (10, 13), (10, 12), (10, 11)];

/// Two posts five cells apart: the second post's Unlimbo raises the four
/// sections between them, facing north-south, and RecalculateWallConnections
/// with force turns the run on from that post: 5 first, 4 in the middle, 6
/// last (north). GetCurrentFrame draws a section at its frame.
#[test]
fn a_second_post_in_reach_raises_an_on_run_of_sections() {
    let (mut sim, rules) = arena();
    let north = sim.spawn_object("POST", "Americans", 10, 10, 0, &rules).unwrap();
    assert_eq!(frames(&sim, &RUN), vec![None; 4]);
    let south = sim.spawn_object("POST", "Americans", 10, 15, 0, &rules).unwrap();
    assert_eq!(frames(&sim, &RUN), vec![Some(5), Some(4), Some(4), Some(6)]);
    let (middle, _) = section(&sim, 10, 12).unwrap();
    let entity = sim.substrate.entities.get(middle).unwrap();
    let object = sim.object_type(entity.type_ref(), &rules).unwrap();
    assert_eq!(object.id, "FENCE");
    assert_eq!(entity.owner(), sim.interner.get("Americans").unwrap());
    assert_eq!(facing_axis(entity.body_facing_current(0)), 0);
    assert_eq!(
        crate::sim::building_art::current_body_frame(entity, object, &rules, 1),
        4
    );
    assert!(alive(&sim, north) && alive(&sim, south));
}

/// Another house's post, or a post past `GuardRange=` cells, raises none.
#[test]
fn posts_of_two_houses_or_out_of_reach_raise_no_sections() {
    let (mut sim, rules) = arena();
    sim.spawn_object("POST", "Americans", 10, 10, 0, &rules).unwrap();
    sim.spawn_object("POST", "Russians", 10, 15, 0, &rules).unwrap();
    assert_eq!(frames(&sim, &RUN), vec![None; 4]);
    sim.spawn_object("POST", "Americans", 19, 10, 0, &rules).unwrap();
    assert_eq!(section(&sim, 15, 10), None, "nine cells apart, reach 8");
}

/// DisableStuff turns a post's run off (12, north-south) through
/// RecalculateWallConnections(0); the run then lets a man in, and a man
/// standing in it burns with Rules' C4Warhead when EnableStuff turns it on
/// again (frames 6/4/4/5 walked from the north post, the same picture).
#[test]
fn a_run_goes_off_with_its_post_and_burns_who_stands_in_it_when_it_returns() {
    let (mut sim, rules) = arena();
    let north = sim.spawn_object("POST", "Americans", 10, 10, 0, &rules).unwrap();
    sim.spawn_object("POST", "Americans", 10, 15, 0, &rules).unwrap();
    let man = sim.spawn_object("E1", "Russians", 13, 12, 0, &rules).unwrap();
    let entry = |sim: &Simulation| {
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        sim.foot_can_enter(
            man,
            cells.lookup((10, 12)),
            crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
            &rules,
            None,
        )
        .unwrap()
    };
    assert_ne!(entry(&sim), 0, "an on section is a building in the way");
    sim.set_building_stuff_enabled(north, false, &rules);
    assert_eq!(frames(&sim, &RUN), vec![Some(12); 4]);
    assert_eq!(entry(&sim), 0, "an off section is ignored");

    let victim = sim.spawn_object("E1", "Russians", 10, 12, 0, &rules).unwrap();
    assert!(alive(&sim, victim));
    sim.set_building_stuff_enabled(north, true, &rules);
    assert_eq!(frames(&sim, &RUN), vec![Some(5), Some(4), Some(4), Some(6)]);
    assert!(!alive(&sim, victim), "C4 burn on activation");
    assert!(alive(&sim, man));
}

/// A Foot entering an on section's cell burns (PerCellProcess
/// `0x004D8CA5`); an off section spares it.
#[test]
fn a_foot_entering_an_on_section_burns() {
    let (mut sim, rules) = arena();
    let north = sim.spawn_object("POST", "Americans", 10, 10, 0, &rules).unwrap();
    sim.spawn_object("POST", "Americans", 10, 15, 0, &rules).unwrap();
    sim.set_building_stuff_enabled(north, false, &rules);
    let man = sim.spawn_object("E1", "Russians", 10, 12, 0, &rules).unwrap();
    let ctx = UninitContext::new(&rules, Some(rules.overlay_registry()));
    sim.laser_fence_burn_foot(man, ctx);
    assert!(alive(&sim, man), "off: no burn");
    // Turning the run on with the man inside already burns him, so set the
    // frame alone to stand for a man walking into an on run.
    let (middle, _) = section(&sim, 10, 12).unwrap();
    sim.set_laser_fence_frame(middle, 4);
    sim.laser_fence_burn_foot(man, ctx);
    assert!(!alive(&sim, man));
}

/// Limbo UnInits a post's run (ConnectWalls(0)); DestructionEffects blows it
/// up (ConnectWalls(1)) when the post is destroyed.
#[test]
fn a_post_taken_away_takes_its_run_with_it() {
    let (mut sim, rules) = arena();
    sim.spawn_object("POST", "Americans", 10, 10, 0, &rules).unwrap();
    let south = sim.spawn_object("POST", "Americans", 10, 15, 0, &rules).unwrap();
    let ctx = UninitContext::new(&rules, Some(rules.overlay_registry()));
    sim.uninit_with_context(south, ctx);
    assert_eq!(frames(&sim, &RUN), vec![None; 4]);

    let (mut sim, rules) = arena();
    sim.spawn_object("POST", "Americans", 10, 10, 0, &rules).unwrap();
    let south = sim.spawn_object("POST", "Americans", 10, 15, 0, &rules).unwrap();
    let sections: Vec<u64> = RUN
        .iter()
        .map(|&(x, y)| section(&sim, x, y).unwrap().0)
        .collect();
    let c4 = sim.interner.intern("C4WH");
    sim.commit_direct_damage_receiver(
        &rules,
        Some(rules.overlay_registry()),
        crate::sim::combat::EntityDamageEvent::direct_receiver(
            south,
            10_000,
            0,
            crate::sim::combat::RAD_NO_ATTACKER,
            None,
            c4,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        ),
        FrameEffects::default(),
    );
    assert!(!alive(&sim, south));
    assert!(sections.iter().all(|&id| !alive(&sim, id)));
}

/// Posts placed inside the placement scope (a map's Unlimbo) raise nothing;
/// InitializeScenarioFromINI's pass (`0x00452D40`) links them afterwards.
#[test]
fn map_posts_link_in_the_scenario_pass() {
    let (mut sim, rules) = arena();
    sim.with_object_placement_scope(|sim| {
        sim.spawn_object("POST", "Americans", 10, 10, 0, &rules).unwrap();
        sim.spawn_object("POST", "Americans", 10, 15, 0, &rules).unwrap();
    });
    assert_eq!(frames(&sim, &RUN), vec![None; 4]);
    sim.with_object_placement_scope(|sim| {
        sim.laser_fence_link_scenario_posts(UninitContext::new(
            &rules,
            Some(rules.overlay_registry()),
        ));
    });
    assert_eq!(frames(&sim, &RUN), vec![Some(5), Some(4), Some(4), Some(6)]);
}

/// An east-west run takes the east-west frames: 2/0/1 walked west and
/// 1/0/2 walked east (the same picture), a lone section 3, and 8 when off.
#[test]
fn an_east_west_run_takes_the_east_west_frames() {
    let (mut sim, rules) = arena();
    let west = sim.spawn_object("POST", "Americans", 10, 10, 0, &rules).unwrap();
    sim.spawn_object("POST", "Americans", 13, 10, 0, &rules).unwrap();
    let run = [(12, 10), (11, 10)];
    assert_eq!(frames(&sim, &run), vec![Some(2), Some(1)], "walked west");
    sim.set_building_stuff_enabled(west, false, &rules);
    assert_eq!(frames(&sim, &run), vec![Some(8), Some(8)]);
    sim.set_building_stuff_enabled(west, true, &rules);
    assert_eq!(frames(&sim, &run), vec![Some(2), Some(1)], "walked east");

    let (mut sim, rules) = arena();
    sim.spawn_object("POST", "Americans", 10, 10, 0, &rules).unwrap();
    sim.spawn_object("POST", "Americans", 12, 10, 0, &rules).unwrap();
    assert_eq!(frames(&sim, &[(11, 10)]), vec![Some(3)]);
}
