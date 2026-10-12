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
/// first), and death and Limbo each take one back.
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

    sim.reset_shroud_for_owner(americans, &rules);
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }
    sim.building_destruction_anims(&rules, id, &mut Vec::new(), |_, _| {});
    assert!(sim.fog.alternate_sight_admissions.is_empty());
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }
    sim.techno_limbo_with_context(id, UninitContext::new(Some(&rules), None));
    for &(x, y) in &cells {
        assert!(sim.fog.is_cell_revealed(americans, x, y), "{x},{y}");
        assert!(!sim.fog.is_cell_visible(americans, x, y), "{x},{y}");
    }
}
