//! The bot interface's fair play: a bot sees what its House's player sees,
//! and more only through a declared cheat.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::bot::{BOTS, BotBrain, BotInfo};

const RULES: &str = "[InfantryTypes]\n0=GI\n[BuildingTypes]\n0=PLANT\n\
    [GI]\nStrength=125\nSpeed=4\nSpeedType=Foot\nSight=3\n\
    Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
    [PLANT]\nStrength=750\nPower=100\nSight=3\n";

fn arena() -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &IniFile::from_str("[PLANT]\nFoundation=2x2\n"),
    )
    .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xB07);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.fog.set_width_for_test(32);
    sim.fog.set_height_for_test(32);
    (sim, rules)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    let id = sim
        .spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"));
    id
}

fn seen(sim: &Simulation, rules: &RuleSet, cheats: &[Cheat], id: u64) -> bool {
    let me = sim.interner().get("Americans").unwrap();
    let view = BotView::new(sim, rules, None, me, cheats);
    view.entity(id).is_some() && view.enemy_objects().any(|e| e.stable_id() == id)
}

#[test]
fn a_fair_bot_sees_enemies_only_where_its_house_has_explored() {
    let (mut sim, rules) = arena();
    spawn(&mut sim, &rules, "GI", "Americans", 3, 3);
    let gi = spawn(&mut sim, &rules, "GI", "Russians", 26, 26);
    let plant = spawn(&mut sim, &rules, "PLANT", "Russians", 20, 26);
    assert!(!seen(&sim, &rules, &[], gi), "unscouted unit");
    assert!(!seen(&sim, &rules, &[], plant), "unscouted building");
    // A scout next to them explores their cells.
    spawn(&mut sim, &rules, "GI", "Americans", 23, 25);
    assert!(seen(&sim, &rules, &[], gi));
    assert!(seen(&sim, &rules, &[], plant));
}

#[test]
fn full_map_vision_is_granted_only_when_declared_and_is_announced() {
    let (mut sim, rules) = arena();
    let gi = spawn(&mut sim, &rules, "GI", "Russians", 26, 26);
    assert!(seen(&sim, &rules, &[Cheat::FullMapVision], gi));
    assert!(!seen(&sim, &rules, &[], gi));
    let cheater = BotInfo {
        name: "x",
        title: "Seer",
        cheats: &[Cheat::FullMapVision],
        make: |_| BotBrain::Idle,
    };
    assert_eq!(
        cheater.announcement().as_deref(),
        Some("Seer (bot) uses: full map vision")
    );
    // The bots this build ships declare nothing, so nothing is granted.
    assert!(
        BOTS.iter()
            .all(|b| b.cheats.is_empty() && b.announcement().is_none())
    );
}

/// A Reveal crate (`MapClass::Reveal @ 0x00577D90` for the picker's House)
/// shows a bot every map cell and, without fog of war, every object on it,
/// as it would show its player.
#[test]
fn a_reveal_crate_shows_a_bot_the_whole_map() {
    let (mut sim, rules) = arena();
    let gi = spawn(&mut sim, &rules, "GI", "Russians", 16, 16);
    let me = sim.interner().get("Americans").unwrap();
    assert!(!seen(&sim, &rules, &[], gi));
    sim.reveal_map_for_owner(me, &rules);
    assert!(seen(&sim, &rules, &[], gi));
}

/// Playtest, 2026-11-14: bots never build right next to an ore spawner, so it
/// has room to grow ore.
#[test]
fn a_bot_keeps_buildings_two_cells_from_an_ore_spawner() {
    let (mut sim, rules) = arena();
    let me = sim.interner().get("Americans").unwrap();
    let near = |sim: &Simulation| {
        BotView::new(sim, &rules, None, me, &[]).near_ore_spawner("PLANT", 10, 10)
    };
    assert!(!near(&sim));
    // The 2x2 PLANT covers 10..=11; a spawner at 13 is two cells off.
    sim.terrain_objects
        .spawning_cells_mut_for_test()
        .insert((13, 12));
    assert!(near(&sim));
    sim.terrain_objects.spawning_cells_mut_for_test().clear();
    sim.terrain_objects
        .spawning_cells_mut_for_test()
        .insert((14, 12));
    assert!(!near(&sim), "three cells off is free");
}
