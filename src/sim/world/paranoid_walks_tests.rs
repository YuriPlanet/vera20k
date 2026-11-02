//! `HouseClass::Computer_Paranoid @ 0x00501640` walks only each bracket's
//! maps that may change ([`ParanoidMaps`]). Each test runs Paranoid on two
//! loads of one save, one walking every map in every bracket as native does,
//! and compares the worlds they end in.
use super::*;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::FrameEffects;

/// [`super::reveal_to_all_tests::world_on`] under the retail Battle rules,
/// whose mode INI sets `AlliesAllowed=yes`: the human current house
/// Americans and the computer houses Russians and Confederation, with an
/// Allied tank and a Russian Tesla Reactor, whose sight `AllyReveal=` shares.
fn world() -> Option<(RuleSet, Simulation)> {
    let rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
    let (rules, mut sim, _) = super::reveal_to_all_tests::world_on(rules);
    sim.spawn_object("MTNK", "Americans", 50, 50, 0, &rules)
        .unwrap();
    sim.spawn_object("NAPOWR", "Russians", 45, 45, 0, &rules)
        .unwrap();
    Some((rules, sim))
}

/// Paranoid on `sim` walking every map in every bracket, then as production
/// does: the sight and border walks each ran. Both end in the same world.
fn walks(sim: &Simulation, rules: &RuleSet) -> (usize, usize) {
    let save = GameSnapshot::save(sim, 0, 0, "paranoid", 0);
    let run = |every| {
        let mut sim = GameSnapshot::load(&save).unwrap().sim;
        WALK_EVERY_MAP.set(every);
        let ((), calls) =
            trace_reveal_calls(|| sim.computer_paranoid(rules, None, FrameEffects::default()));
        WALK_EVERY_MAP.set(false);
        assert_eq!(
            sim.house_alliances().as_map().len(),
            2,
            "each computer allied the other"
        );
        (
            sim.state_hash(),
            calls.walks.len() + calls.border_scans.len(),
        )
    };
    let (every, every_walks) = run(true);
    let (hash, walks) = run(false);
    assert_eq!(hash, every);
    (walks, every_walks)
}

/// A Russian Gap Generator turns friendly on each new ally's map
/// (`0x006FB170`/`0x006FB470` take the friendly arm) as the Reactor's sight
/// opens there, and the brackets after leave the maps they settled unwalked.
#[test]
fn paranoid_skips_settled_maps_around_a_gap_generator() {
    let Some((rules, mut sim)) = world() else {
        return;
    };
    let gap = sim
        .spawn_object("GAGAP", "Russians", 48, 45, 0, &rules)
        .unwrap();
    crate::sim::power_system::tick_power_states(
        &mut sim.power_states,
        &mut sim.substrate.entities,
        &rules,
        &sim.interner,
        sim.session.binary_frame(),
    );
    assert_eq!(sim.building_operational_state(gap, &rules), Some(true));
    let (walks, every) = walks(&sim, &rules);
    assert!(walks < every, "{walks} of {every} walks");
}

/// A Russian Iron Curtain's `RevealToAll=` sight counts again on the current
/// house's map in each bracket (`0x004ADE68`), so every bracket walks it.
#[test]
fn paranoid_counts_reveal_to_all_sight_in_every_bracket() {
    let Some((rules, mut sim)) = world() else {
        return;
    };
    sim.spawn_object("NAIRON", "Russians", 48, 45, 0, &rules)
        .unwrap();
    let (walks, every) = walks(&sim, &rules);
    assert_eq!(walks, every);
}

/// Under `FogOfWar=`, a reveal of a map's fresh cell cleans its fog
/// (`CellClass::CleanFog @ 0x00486BF0`), state no one map owns: the bracket
/// after walks every map.
#[test]
fn paranoid_under_fog_of_war_matches_every_map() {
    let Some((rules, mut sim)) = world() else {
        return;
    };
    sim.session.game_options_mut_for_test().fog_of_war = true;
    walks(&sim, &rules);
}
