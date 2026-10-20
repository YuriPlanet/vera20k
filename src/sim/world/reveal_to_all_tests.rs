//! `RevealToAll=` (TechnoType `+0x5EE`) through its production owners: a
//! computer house's building opening, a capture, the whole-map transitions,
//! its death and Limbo.
use super::*;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::{FrameEffects, SimSoundEvent};

/// The building's top-left cell, well inside the map's `Size=` diamond
/// (`rx + ry > 64` in this Size64x64 diamond).
const SITE: (u16, u16) = (45, 45);
/// A cell on the map that no source in these tests sees.
const FAR: (u16, u16) = (60, 60);

/// Retail rules on a Size64x64 diamond in128x128 storage: the human current house
/// `Americans` and the computer houses `Russians` and `Confederation`.
fn world() -> Option<(RuleSet, Simulation, InternedId)> {
    use crate::sim::superweapon::chronosphere_tests::{retail_rules_binding, world_with};
    let (rules, mut sim, americans) = world_with(retail_rules_binding(&[])?, 128, &[]);
    //578350 starts at(1,SizeW), not at a rectangular grid's first cell.
    //Native Size64x64 spans packed coordinates1..127, just as the existing
    //shroud_refresh Size6x4 fixture uses10x10 storage for its allocated walk.
    sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
        theater: "TEMPERATE".into(),
        fill: "Clear".into(),
        level: 0,
        width: 64,
        height: 64,
        local_left: 0,
        local_top: 0,
        local_width: 64,
        local_height: 64,
    });
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let confederation = sim.interner.intern("Confederation");
    sim.houses
        .register(HouseState::new(confederation, 1, None, false, 0, 10));
    sim.session.set_current_house_for_test(Some(americans));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.fog = vision::FogState::new(128, 128);
    let allocated: std::collections::BTreeSet<_> = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .native_cell_iterator(64)
        .map(|cell| (cell.rx, cell.ry))
        .collect();
    assert!(allocated.contains(&SITE) && allocated.contains(&FAR));
    assert!(!allocated.contains(&(0, 0)));
    Some((rules, sim, americans))
}

fn radar_events(sim: &Simulation) -> Vec<RadarEventRequest> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::RadarEvent { radar } => Some(*radar),
            _ => None,
        })
        .collect()
}

const OPENED: RadarEventRequest =
    RadarEventRequest::new(RadarEventType::ConstructionComplete, SITE.0, SITE.1);

/// The cells the computer house's Iron Curtain shows the current house.
fn alternate_cells(sim: &Simulation, id: u64, viewer: InternedId) -> Vec<(u16, u16)> {
    sim.fog.alternate_sight_admissions_for_test()[&(id, viewer)]
        .cells
        .clone()
}

/// `viewer`'s shroud counter at a cell (CellClass `+0x130`'s on the native
/// map): 1 shrouded, 0 explored, -n under n sight contributions.
fn counter(sim: &Simulation, viewer: InternedId, (x, y): (u16, u16)) -> i32 {
    sim.fog.by_owner[&viewer].shroud_knowledge_raw()
        [usize::from(y) * usize::from(sim.fog.width) + usize::from(x)]
    .counter
}

#[test]
fn a_computer_houses_opening_shows_the_current_house_its_sight_until_it_dies() {
    let Some((rules, mut sim, americans)) = world() else {
        return;
    };
    let id = sim
        .spawn_object("NAIRON", "Russians", SITE.0, SITE.1, 0, &rules)
        .unwrap();
    // Its Unlimbo's DiscoveredBy(owner) opened it.
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .building_art
            .actually_placed()
    );
    assert_eq!(radar_events(&sim), [OPENED]);
    let keys: Vec<_> = sim
        .fog
        .alternate_sight_admissions_for_test()
        .keys()
        .copied()
        .collect();
    assert_eq!(keys, [(id, americans)]);
    // `Sight=4`, on flat ground with no veterancy.
    assert_eq!(
        sim.fog.alternate_sight_admissions_for_test()[&(id, americans)].radius,
        4
    );
    let cells = alternate_cells(&sim, id, americans);
    assert!(cells.contains(&SITE));
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }
    assert!(!sim.fog.is_cell_revealed(americans, FAR.0, FAR.1));

    // The frame pass keeps the owner's own sight, which its Unlimbo opened
    // on its map, and leaves this one in place.
    let russians = sim.interner.get("Russians").unwrap();
    let config = sim.sight_reveal_config(Some(&rules));
    sim.refresh_fog(&config, Some(&rules));
    assert!(sim.fog.sight_is_latched(id, russians));
    assert!(!sim.fog.sight_is_latched(id, americans));
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }

    // A save keeps it, and the lockstep hash covers it.
    let loaded = GameSnapshot::load(&GameSnapshot::save(&sim, 0, 0, "reveal_to_all", 0))
        .unwrap()
        .sim;
    assert_eq!(
        loaded.fog.alternate_sight_admissions_for_test(),
        sim.fog.alternate_sight_admissions_for_test()
    );
    let hash = sim.state_hash();
    let admitted = std::mem::take(sim.fog.alternate_sight_admissions_mut_for_test());
    assert_ne!(sim.state_hash(), hash);
    *sim.fog.alternate_sight_admissions_mut_for_test() = admitted;
    assert_eq!(sim.state_hash(), hash);

    // DestructionEffects step 5 takes the sight back; the cells stay explored.
    sim.building_destruction_anims(&rules, id, &mut Vec::new(), |_, _| {});
    assert!(sim.fog.alternate_sight_admissions_for_test().is_empty());
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_revealed(americans, x, y), "{x},{y}");
        assert!(!sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }
    sim.techno_limbo_with_context(id, UninitContext::new(Some(&rules), None));
    assert!(sim.fog.alternate_sight_admissions_for_test().is_empty());
}

#[test]
fn captures_keep_the_sight_and_a_shroud_reset_shows_it_again() {
    let Some((rules, mut sim, americans)) = world() else {
        return;
    };
    let confederation = sim.interner.intern("Confederation");
    let id = sim
        .spawn_object("NAIRON", "Russians", SITE.0, SITE.1, 0, &rules)
        .unwrap();
    let admitted = sim.fog.alternate_sight_admissions_for_test().clone();
    let cells = alternate_cells(&sim, id, americans);

    // ChangeOwner's ReReveal(0, 0, 0, 0) keeps it; the reopening for another
    // computer house raises the radar event alone.
    sim.sound_events.clear();
    sim.change_owner_with_rules(id, confederation, &rules, None, FrameEffects::default());
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().owner(),
        confederation
    );
    assert_eq!(radar_events(&sim), [OPENED]);
    assert_eq!(sim.fog.alternate_sight_admissions_for_test(), &admitted);

    // A whole-map reveal, then a reset: the reset covers the map again except
    // where the re-admission callback shows the alternate sight.
    sim.reveal_map_for_owner(americans, &rules);
    assert!(sim.fog.is_cell_revealed(americans, FAR.0, FAR.1));
    sim.reset_shroud_for_owner(americans, &rules);
    assert!(!sim.fog.is_cell_revealed(americans, FAR.0, FAR.1));
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }
    assert_eq!(sim.fog.alternate_sight_admissions_for_test(), &admitted);

    // The human's capture raises no radar event and keeps the sight too.
    sim.sound_events.clear();
    sim.change_owner_with_rules(id, americans, &rules, None, FrameEffects::default());
    assert!(radar_events(&sim).is_empty());
    assert_eq!(sim.fog.alternate_sight_admissions_for_test(), &admitted);

    // A human's building skips step 5; its Limbo takes the sight back.
    sim.building_destruction_anims(&rules, id, &mut Vec::new(), |_, _| {});
    assert_eq!(sim.fog.alternate_sight_admissions_for_test(), &admitted);
    sim.techno_limbo_with_context(id, UninitContext::new(Some(&rules), None));
    assert!(sim.fog.alternate_sight_admissions_for_test().is_empty());
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_revealed(americans, x, y), "{x},{y}");
        assert!(!sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }
}

/// An `AllyReveal=` ally's building: the current house's map holds the sight
/// the ally shares and the alternate one. A Reset_Shroud re-admits only the
/// shared sight (the allied-building branch, `0x004ADDB2..0x004ADDE4`, comes
/// first). Death's ReReveal(0, 0, 1, PlayerPtr) takes it back, clearing the
/// latch it was admitted under, and Limbo's leaves the explored cells as
/// they are.
#[test]
fn an_allys_building_shows_both_sights_and_takes_both_back() {
    let Some((rules, mut sim, americans)) = world() else {
        return;
    };
    sim.house_alliances = crate::map::houses::HouseAllianceMap::from([
        (
            "AMERICANS".to_string(),
            std::collections::BTreeSet::from(["RUSSIANS".to_string()]),
        ),
        (
            "RUSSIANS".to_string(),
            std::collections::BTreeSet::from(["AMERICANS".to_string()]),
        ),
    ]);
    let id = sim
        .spawn_object("NAIRON", "Russians", SITE.0, SITE.1, 0, &rules)
        .unwrap();
    let config = sim.sight_reveal_config(Some(&rules));
    sim.refresh_fog(&config, Some(&rules));
    assert!(sim.fog.sight_is_latched(id, americans));
    let cells = alternate_cells(&sim, id, americans);

    for &cell in &cells {
        assert_eq!(counter(&sim, americans, cell), -2, "{cell:?}");
    }
    sim.reset_shroud_for_owner(americans, &rules);
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
        assert_eq!(counter(&sim, americans, (x, y)), -1, "{x},{y}");
    }
    sim.building_destruction_anims(&rules, id, &mut Vec::new(), |_, _| {});
    assert!(sim.fog.alternate_sight_admissions_for_test().is_empty());
    assert!(
        !sim.fog
            .sight_admissions_for_test()
            .iter()
            .any(|(key, source)| key.0 == id && source.latched)
    );
    for &(x, y) in &cells {
        assert!(!sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
        assert_eq!(counter(&sim, americans, (x, y)), 0, "{x},{y}");
    }
    sim.techno_limbo_with_context(id, UninitContext::new(Some(&rules), None));
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_revealed(americans, x, y), "{x},{y}");
        assert!(!sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
        assert_eq!(counter(&sim, americans, (x, y)), 0, "{x},{y}");
    }
}

/// A computer house's building that dies takes its sight off the current
/// house's map twice: DestructionEffects' ReReveal(0, 0, 1, PlayerPtr)
/// clears the latch (`0x0070B202`) and releases it, and Limbo's
/// `RevealToAll=` branch releases the stored footprint again with the latch
/// clear (`0x0070B234..0x0070B26C`). A GI of the current house standing in
/// it keeps the cell in sight, but its contribution is gone from the
/// counter. The building dies to a lethal hit through ReceiveDamage's commit,
/// which runs both before any sight refresh.
#[test]
fn a_computer_buildings_death_takes_its_sight_off_the_current_houses_map_twice() {
    let Some((rules, mut sim, americans)) = world() else {
        return;
    };
    let id = sim
        .spawn_object("NAIRON", "Russians", SITE.0, SITE.1, 0, &rules)
        .unwrap();
    let post = *alternate_cells(&sim, id, americans)
        .iter()
        .max_by_key(|cell| cell.1)
        .unwrap();
    let gi = sim
        .spawn_object("E1", "Americans", post.0, post.1, 0, &rules)
        .unwrap();
    let config = sim.sight_reveal_config(Some(&rules));
    sim.refresh_fog(&config, Some(&rules));
    assert!(
        sim.fog.sight_admissions_for_test()[&(gi, americans)]
            .cells
            .contains(&post)
    );
    assert_eq!(counter(&sim, americans, post), -2);

    let warhead = sim.interner.intern("Super");
    let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
        id,
        100_000,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        warhead,
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(&rules, None, &[hit], FrameEffects::default());
    assert!(
        sim.substrate
            .entities
            .get(id)
            .is_none_or(|entity| entity.lifecycle.in_limbo())
    );
    assert_eq!(counter(&sim, americans, post), 0);
    assert!(sim.fog.is_cell_visible(americans, post.0, post.1));
    assert!(sim.fog.alternate_sight_admissions_for_test().is_empty());
    assert!(
        !sim.fog
            .sight_admissions_for_test()
            .iter()
            .any(|(key, source)| key.0 == id && source.latched)
    );
}

/// A sold building leaves through Limbo alone. Its `RevealToAll=` branch
/// releases the footprint for the current house in place of the owner's
/// (`0x006F6AEF..0x006F6B06`), so the sight an `AllyReveal=` ally shared
/// stays counted on the current house's map.
#[test]
fn a_sold_allys_building_leaves_its_shared_sight_counted() {
    let Some((rules, mut sim, americans)) = world() else {
        return;
    };
    sim.house_alliances = crate::map::houses::HouseAllianceMap::from([
        (
            "AMERICANS".to_string(),
            std::collections::BTreeSet::from(["RUSSIANS".to_string()]),
        ),
        (
            "RUSSIANS".to_string(),
            std::collections::BTreeSet::from(["AMERICANS".to_string()]),
        ),
    ]);
    let id = sim
        .spawn_object("NAIRON", "Russians", SITE.0, SITE.1, 0, &rules)
        .unwrap();
    let config = sim.sight_reveal_config(Some(&rules));
    sim.refresh_fog(&config, Some(&rules));
    let cells = alternate_cells(&sim, id, americans);
    for &cell in &cells {
        assert_eq!(counter(&sim, americans, cell), -2, "{cell:?}");
    }

    sim.techno_limbo_with_context(id, UninitContext::new(Some(&rules), None));
    sim.refresh_fog(&config, Some(&rules));
    assert!(sim.fog.alternate_sight_admissions_for_test().is_empty());
    assert!(
        !sim.fog
            .sight_admissions_for_test()
            .iter()
            .any(|(key, source)| key.0 == id && source.latched)
    );
    for &(x, y) in &cells {
        assert_eq!(counter(&sim, americans, (x, y)), -1, "{x},{y}");
        assert!(sim.fog.is_cell_revealed(americans, x, y), "{x},{y}");
        assert!(!sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }
}

/// A computer house's Iron Curtain placed during play builds up unopened. A
/// Reset_Shroud of the current house shows it its sight all the same: the
/// re-admission callback's `RevealToAll=` branch (`0x004ADE26..0x004ADE68`)
/// takes every computer house's building of the type. The opening then
/// counts it again (`0x00446816..0x00446829`), and death and Limbo take both
/// back.
#[test]
fn a_shroud_reset_shows_a_computer_superweapon_still_building_up() {
    use crate::sim::world::PlacementEvidence;
    let Some((rules, mut sim, americans)) = world() else {
        return;
    };
    // Original5678E0 rejects noncurrent ordinary sight before its Cell
    // callbacks. Thus no invented owner-plane Discover can open this AI
    // building before Unlimbo queues Construction (6F6E2A/44D6BD..D0) and
    // the real later owner discovery440E76 leaves it shut.
    let id = sim
        .construct_object_limbo_at_height(
            "NAIRON",
            "Russians",
            SITE.0,
            SITE.1,
            0,
            0,
            &rules,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    let (revealed, calls) = trace_reveal_calls(|| {
        sim.reveal_constructed_object_at_height(
            id,
            SITE.0,
            SITE.1,
            0,
            0,
            PlacementEvidence::EvaluateMark,
            &rules,
            FrameEffects::default(),
        )
    });
    assert_eq!(revealed, Some(id));
    assert!(
        calls.notifications.is_empty(),
        "noncurrent logical sight is not a native5865F0 call"
    );
    let russians = sim.interner.get("Russians").unwrap();
    assert!(
        sim.fog.sight_is_latched(id, russians),
        "70AF50 caches before Map admission"
    );
    assert!(
        sim.fog.is_cell_revealed(russians, SITE.0, SITE.1),
        "logical query projection is retained"
    );
    let building = sim.substrate.entities.get(id).unwrap();
    assert_eq!((building.position.rx(), building.position.ry()), SITE);
    assert!(building.building_up());
    assert!(!building.building_art.actually_placed());
    assert!(sim.fog.alternate_sight_admissions_for_test().is_empty());

    sim.reset_shroud_for_owner(americans, &rules);
    let cells = alternate_cells(&sim, id, americans);
    assert!(cells.contains(&SITE));
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
        assert_eq!(counter(&sim, americans, (x, y)), -1, "{x},{y}");
    }
    assert!(!sim.fog.is_cell_revealed(americans, FAR.0, FAR.1));

    sim.grand_opening(id, false, false, &rules, None, FrameEffects::default());
    for &cell in &cells {
        assert_eq!(counter(&sim, americans, cell), -2, "{cell:?}");
    }

    sim.building_destruction_anims(&rules, id, &mut Vec::new(), |_, _| {});
    sim.techno_limbo_with_context(id, UninitContext::new(Some(&rules), None));
    assert!(sim.fog.alternate_sight_admissions_for_test().is_empty());
    for &(x, y) in &cells {
        assert_eq!(counter(&sim, americans, (x, y)), 0, "{x},{y}");
        assert!(sim.fog.is_cell_revealed(americans, x, y), "{x},{y}");
        assert!(!sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }
}

#[test]
fn native_action51_keeps_null_house241_and_gates_foreign_map_changes() {
    // Map577C90 --reshroud corpus: full branch/allocated loop with declared
    // no-source release/reentry sinks. Production uses the shared Fog owners;
    // this is not an occupied-map/Tag callback or whole-ALL01 parity claim.
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_reshroud.json",
    ))
    .unwrap();
    for row in corpus["controls"].as_array().unwrap() {
        let Some((rules, mut sim, player)) = world() else {
            return;
        };
        let foreign = sim.interner.get("Russians").unwrap();
        let (fog, terrain) = vision::native_map_cell_fixture(&row["topology"], player);
        sim.fog = fog;
        sim.resolved_terrain = Some(terrain);
        let width = row["size_width"].as_u64().unwrap() as u32;
        sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
            theater: "TEMPERATE".into(),
            fill: "Clear".into(),
            level: 0,
            width,
            height: width,
            local_left: 0,
            local_top: 0,
            local_width: width,
            local_height: width,
        });
        for (name, owner) in [("player", player), ("foreign", foreign)] {
            sim.houses
                .get_mut(&owner)
                .unwrap()
                .set_map_is_clear_for_test(row["houses_before"][name]["clear"] == 1);
            assert_eq!(row["houses_before"][name]["revealed"], 1);
            sim.fog.admit_whole_map_reveal(owner);
        }
        let rng_before = (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        let incoming = match row["incoming"].as_str() {
            Some("player") => Some(player),
            Some("foreign") => Some(foreign),
            None => None,
            other => panic!("unexpected native incoming House {other:?}"),
        };
        sim.trigger_reshroud_map(incoming, &rules);
        for (name, owner) in [("player", player), ("foreign", foreign)] {
            assert_eq!(
                sim.houses[&owner].map_is_clear(),
                row["houses"][name]["clear"] == 1,
                "{} {name}241",
                row["name"]
            );
            assert_eq!(
                sim.fog.whole_map_revealed_for_owner(owner),
                row["houses"][name]["revealed"] == 1,
                "{} {name}240",
                row["name"]
            );
        }
        for cell in row["cells"].as_array().unwrap() {
            vision::assert_native_cell_history(
                &sim.fog,
                player,
                cell,
                sim.resolved_terrain.as_ref().unwrap(),
                false,
            );
        }
        assert_eq!(
            (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state(),
            ),
            rng_before,
            "{} must not draw RNG with empty callbacks",
            row["name"]
        );
        assert!(sim.substrate.entities.is_empty());
        assert_eq!(row["rng_before"], row["rng_after"]);
        assert!(row["rng_calls"].as_array().unwrap().is_empty());
    }
}
