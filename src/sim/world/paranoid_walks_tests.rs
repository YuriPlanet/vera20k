//! `HouseClass::Computer_Paranoid @ 0x00501640` walks only each bracket's
//! maps that may change ([`ParanoidMaps`]). Each test builds one world twice
//! and runs Paranoid on both, one walking every map in every bracket as
//! native does, and compares the worlds they end in.
use super::*;
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

/// Paranoid on [`world`] and `setup`, built twice: walking every map in
/// every bracket, then as production does. Both end in the same world;
/// returns how many sight and border walks each ran.
fn walks(setup: impl Fn(&RuleSet, &mut Simulation)) -> Option<(usize, usize)> {
    let run = |every| {
        let (rules, mut sim) = world()?;
        setup(&rules, &mut sim);
        WALK_EVERY_MAP.set(every);
        let ((), calls) =
            trace_reveal_calls(|| sim.computer_paranoid(&rules, None, FrameEffects::default()));
        WALK_EVERY_MAP.set(false);
        assert_eq!(
            sim.house_alliances().as_map().len(),
            2,
            "each computer allied the other"
        );
        Some((
            sim.state_hash(),
            calls.walks.len() + calls.border_scans.len(),
        ))
    };
    let (every, every_walks) = run(true)?;
    let (hash, walks) = run(false)?;
    assert_eq!(hash, every);
    Some((walks, every_walks))
}

/// A Russian Gap Generator turns friendly on each new ally's map
/// (`0x006FB170`/`0x006FB470` take the friendly arm) as the Reactor's sight
/// opens there, and the brackets after leave the maps they settled unwalked.
#[test]
fn paranoid_skips_settled_maps_around_a_gap_generator() {
    let Some((walks, every)) = walks(|rules, sim| {
        let gap = sim
            .spawn_object("GAGAP", "Russians", 48, 45, 0, rules)
            .unwrap();
        crate::sim::power_system::tick_power_states(
            &mut sim.power_states,
            &mut sim.substrate.entities,
            rules,
            &sim.interner,
            sim.session.binary_frame(),
        );
        assert_eq!(sim.building_operational_state(gap, rules), Some(true));
    }) else {
        return;
    };
    assert!(walks < every, "{walks} of {every} walks");
}

/// A Russian Iron Curtain's `RevealToAll=` sight counts again on the current
/// house's map in each bracket (`0x004ADE68`), so every bracket walks it.
#[test]
fn paranoid_counts_reveal_to_all_sight_in_every_bracket() {
    let Some((walks, every)) = walks(|rules, sim| {
        sim.spawn_object("NAIRON", "Russians", 48, 45, 0, rules)
            .unwrap();
    }) else {
        return;
    };
    assert_eq!(walks, every);
}

/// Off the map's diamond, a walk's lookups stamp the shared Dummy cell
/// (`NativeCellQuery::lookup`): a Russian reactor's on the computers' maps,
/// then an American tank's on the human's. The tank's target holds the
/// Dummy, so its coordinate ends where native leaves it.
#[test]
fn paranoid_leaves_a_held_dummy_cell_where_native_does() {
    walks(|rules, sim| {
        // Only the Size64x64 diamond is real, as a map load leaves it.
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        let diamond: Vec<_> = terrain
            .native_cell_iterator(64)
            .map(|cell| (cell.rx, cell.ry))
            .collect();
        terrain.test_set_native_allocated_cells(&diamond);
        // Their sight reaches past the diamond's edges.
        let tank = sim.with_object_placement_scope(|sim| {
            sim.spawn_object("NAPOWR", "Russians", 40, 98, 0, rules)
                .unwrap();
            sim.spawn_object("MTNK", "Americans", 98, 40, 0, rules)
                .unwrap()
        });
        let target = Some(crate::sim::combat::TargetKind::Cell(200, 200));
        let entity = sim.substrate.entities.get_mut(tank).unwrap();
        entity.attack_target.set_for_test(target);
    });
}

/// Under `FogOfWar=`, a reveal of a map's fresh cell cleans its fog
/// (`CellClass::CleanFog @ 0x00486BF0`), state no one map owns: the bracket
/// after walks every map.
#[test]
fn paranoid_under_fog_of_war_matches_every_map() {
    walks(|_, sim| sim.session.game_options_mut_for_test().fog_of_war = true);
}
