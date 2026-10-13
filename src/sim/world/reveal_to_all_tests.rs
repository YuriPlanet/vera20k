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
/// (`rx + ry > 64` in this 64x64 storage).
const SITE: (u16, u16) = (45, 45);
/// A cell on the map that no source in these tests sees.
const FAR: (u16, u16) = (60, 60);

/// Retail rules on a clear 64x64 skirmish map: the human current house
/// `Americans` and the computer houses `Russians` and `Confederation`.
fn world() -> Option<(RuleSet, Simulation, InternedId)> {
    use crate::sim::superweapon::chronosphere_tests::{retail_rules_binding, world_with};
    let (rules, mut sim, americans) = world_with(retail_rules_binding(&[])?, 64, &[]);
    let confederation = sim.interner.intern("Confederation");
    sim.houses.insert(
        confederation,
        HouseState::new(confederation, 1, None, false, 0, 10),
    );
    sim.session.house_order.push(confederation);
    sim.session.current_house = Some(americans);
    sim.session.game_mode_nonzero = true;
    sim.fog.width = 64;
    sim.fog.height = 64;
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
    sim.fog.alternate_sight_admissions[&(id, viewer)]
        .cells
        .clone()
}

/// `viewer`'s shroud counter at a cell (CellClass `+0x130`'s on the native
/// map): 1 shrouded, 0 explored, -n under n sight contributions.
fn counter(sim: &Simulation, viewer: InternedId, (x, y): (u16, u16)) -> i32 {
    sim.fog.by_owner[&viewer].shroud_knowledge_raw()[usize::from(y) * 64 + usize::from(x)].counter
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
            .building_actually_placed
    );
    assert_eq!(radar_events(&sim), [OPENED]);
    let keys: Vec<_> = sim.fog.alternate_sight_admissions.keys().copied().collect();
    assert_eq!(keys, [(id, americans)]);
    // `Sight=4`, on flat ground with no veterancy.
    assert_eq!(
        sim.fog.alternate_sight_admissions[&(id, americans)].radius,
        4
    );
    let cells = alternate_cells(&sim, id, americans);
    assert!(cells.contains(&SITE));
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }
    assert!(!sim.fog.is_cell_revealed(americans, FAR.0, FAR.1));

    // The per-frame sight refresh admits the owner's own sight on its map and
    // leaves this one in place.
    let russians = sim.interner.get("Russians").unwrap();
    let config = sim.sight_reveal_config(Some(&rules));
    sim.refresh_fog(&config, Some(&rules));
    assert!(sim.fog.sight_admissions.contains_key(&(id, russians)));
    assert!(!sim.fog.sight_admissions.contains_key(&(id, americans)));
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }

    // A save keeps it, and the lockstep hash covers it.
    let loaded = GameSnapshot::load(&GameSnapshot::save(&sim, 0, 0, "reveal_to_all", 0))
        .unwrap()
        .sim;
    assert_eq!(
        loaded.fog.alternate_sight_admissions,
        sim.fog.alternate_sight_admissions
    );
    let hash = sim.state_hash();
    let admitted = std::mem::take(&mut sim.fog.alternate_sight_admissions);
    assert_ne!(sim.state_hash(), hash);
    sim.fog.alternate_sight_admissions = admitted;
    assert_eq!(sim.state_hash(), hash);

    // DestructionEffects step 5 takes the sight back; the cells stay explored.
    sim.building_destruction_anims(&rules, id, &mut Vec::new(), |_, _| {});
    assert!(sim.fog.alternate_sight_admissions.is_empty());
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_revealed(americans, x, y), "{x},{y}");
        assert!(!sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }
    sim.techno_limbo_with_context(id, UninitContext::new(Some(&rules), None));
    assert!(sim.fog.alternate_sight_admissions.is_empty());
}

#[test]
fn a_human_houses_opening_shows_no_one_else_its_sight() {
    let Some((rules, mut sim, _)) = world() else {
        return;
    };
    let id = sim
        .spawn_object("NAIRON", "Americans", SITE.0, SITE.1, 0, &rules)
        .unwrap();
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .building_actually_placed
    );
    assert!(radar_events(&sim).is_empty());
    assert!(sim.fog.alternate_sight_admissions.is_empty());
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
    let admitted = sim.fog.alternate_sight_admissions.clone();
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
    assert_eq!(sim.fog.alternate_sight_admissions, admitted);

    // A whole-map reveal, then a reset: the reset covers the map again except
    // where the re-admission callback shows the alternate sight.
    sim.reveal_map_for_owner(americans, &rules);
    assert!(sim.fog.is_cell_revealed(americans, FAR.0, FAR.1));
    sim.reset_shroud_for_owner(americans, &rules);
    assert!(!sim.fog.is_cell_revealed(americans, FAR.0, FAR.1));
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }
    assert_eq!(sim.fog.alternate_sight_admissions, admitted);

    // The human's capture raises no radar event and keeps the sight too.
    sim.sound_events.clear();
    sim.change_owner_with_rules(id, americans, &rules, None, FrameEffects::default());
    assert!(radar_events(&sim).is_empty());
    assert_eq!(sim.fog.alternate_sight_admissions, admitted);

    // A human's building skips step 5; its Limbo takes the sight back.
    sim.building_destruction_anims(&rules, id, &mut Vec::new(), |_, _| {});
    assert_eq!(sim.fog.alternate_sight_admissions, admitted);
    sim.techno_limbo_with_context(id, UninitContext::new(Some(&rules), None));
    assert!(sim.fog.alternate_sight_admissions.is_empty());
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
    assert!(sim.fog.sight_admissions.contains_key(&(id, americans)));
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
    assert!(sim.fog.alternate_sight_admissions.is_empty());
    assert!(!sim.fog.sight_admissions.keys().any(|key| key.0 == id));
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
        sim.fog.sight_admissions[&(gi, americans)]
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
            .is_none_or(|entity| entity.lifecycle.in_limbo)
    );
    assert_eq!(counter(&sim, americans, post), 0);
    assert!(sim.fog.is_cell_visible(americans, post.0, post.1));
    assert!(sim.fog.alternate_sight_admissions.is_empty());
    assert!(!sim.fog.sight_admissions.keys().any(|key| key.0 == id));
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
    assert!(sim.fog.alternate_sight_admissions.is_empty());
    assert!(!sim.fog.sight_admissions.keys().any(|key| key.0 == id));
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
    // Its Unlimbo queues Construction (`0x006F6E2A`, `0x0044D6BD..0x0044D6D0`)
    // before its DiscoveredBy(owner), which therefore leaves it shut.
    let id = sim
        .construct_object_limbo_at_height("NAIRON", "Russians", SITE.0, SITE.1, 0, 0, &rules)
        .unwrap();
    assert_eq!(
        sim.reveal_constructed_object_at_height(
            id,
            SITE.0,
            SITE.1,
            0,
            0,
            PlacementEvidence::EvaluateMark,
            &rules,
            FrameEffects::default(),
        ),
        Some(id)
    );
    let building = sim.substrate.entities.get(id).unwrap();
    assert_eq!((building.position.rx, building.position.ry), SITE);
    assert!(building.building_up());
    assert!(!building.building_actually_placed);
    assert!(sim.fog.alternate_sight_admissions.is_empty());

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
    assert!(sim.fog.alternate_sight_admissions.is_empty());
    for &(x, y) in &cells {
        assert_eq!(counter(&sim, americans, (x, y)), 0, "{x},{y}");
        assert!(sim.fog.is_cell_revealed(americans, x, y), "{x},{y}");
        assert!(!sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }
}

/// A computer house that captures the human's Iron Curtain reopens it
/// without showing its sight (`Grand_Opening(1)`). A Reset_Shroud of the
/// current house shows it, as it does any computer house's.
#[test]
fn a_shroud_reset_shows_a_superweapon_a_computer_captured_from_the_human() {
    let Some((rules, mut sim, americans)) = world() else {
        return;
    };
    let russians = sim.interner.get("Russians").unwrap();
    let id = sim
        .spawn_object("NAIRON", "Americans", SITE.0, SITE.1, 0, &rules)
        .unwrap();
    sim.change_owner_with_rules(id, russians, &rules, None, FrameEffects::default());
    assert_eq!(radar_events(&sim), [OPENED]);
    assert!(sim.fog.alternate_sight_admissions.is_empty());

    sim.reset_shroud_for_owner(americans, &rules);
    let cells = alternate_cells(&sim, id, americans);
    assert!(cells.contains(&SITE));
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
        assert_eq!(counter(&sim, americans, (x, y)), -1, "{x},{y}");
    }
    assert!(!sim.fog.is_cell_revealed(americans, FAR.0, FAR.1));
}
