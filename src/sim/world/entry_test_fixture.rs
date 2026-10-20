//! Shared native-shaped world fixture for live cell entry and bridge publication tests.
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::{ini_parser::IniFile, locomotor_type::MovementZone, ruleset::RuleSet};
use crate::sim::{
    bridge_state::BridgeRuntimeState,
    overlay_grid::OverlayGrid,
    pathfinding::{PathGrid, zone_map::ZoneGrid},
    world::Simulation,
};
use std::sync::Arc;

pub(crate) fn fixture() -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
) {
    fixture_with_rules("")
}

pub(crate) fn fixture_with_rules(
    extra: &str,
) -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
) {
    fixture_with_rules_and_fixed_art(extra, &IniFile::from_str(""))
}

/// Build the ordinary entry fixture with explicit ART available to native type reads.
pub(crate) fn fixture_with_rules_and_fixed_art(
    extra: &str,
    art: &IniFile,
) -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
) {
    // Every part but the Simulation is a function of `extra` and `art`, so a
    // test process builds them once per pair. Each copy of the terrain gets
    // its own Dummy cell, in the state the zone build left.
    let key = (extra.to_owned(), art.content_hash());
    let parts = crate::test_fixture::once_per_process(key, || {
        Some(Arc::new(FixtureParts::build(extra, art)))
    })
    .expect("entry fixture parts");
    let mut terrain = parts.terrain.clone();
    terrain.bind_shared_cell_dummy(parts.terrain.shared_cell_dummy().fork_query_identity());
    let rules = parts.rules.clone().with_fresh_serial();
    let mut sim = Simulation::with_seed(31);
    // Match production initialization: every registered type exists before
    // building the derived table, including types first spawned after load.
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.playfield_bounds = Some(parts.bounds);
    // Map Size height, which map load installs with the bounds
    // (`install_playfield_from_map_header`).
    sim.playfield_size_height = Some(16);
    sim.session.set_map_width_for_test(33);
    sim.session.set_map_height_for_test(33);
    sim.overlay_grid = Some(OverlayGrid::new(33, 33));
    sim.bridge_state = Some(parts.bridges.clone());
    sim.zone_grid = Some(parts.zones.clone());
    sim.path_grid = Some(Arc::new(parts.path.clone()));
    sim.install_resolved_terrain_for_new_map(terrain);
    // Map load also sizes the ore queue stores from the same `[Map] Size`
    // rect (`initialize_native_tiberium_queues`); this map holds no ore.
    let frame = sim.session.binary_frame();
    sim.ore_growth_state.reset_native_tiberium_classes_for_rect(
        (16, 16),
        rules.tiberium_types.len(),
        frame,
    );
    for p in [(15, 15), (16, 15), (17, 14), (17, 15), (17, 16)] {
        assert!(crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (p.0, p.1),
            sim.playfield_bounds,
            sim.resolved_terrain.as_ref()
        ));
    }
    (sim, rules, parts.registry.clone())
}

/// The rules, overlay registry and map of one entry fixture.
struct FixtureParts {
    rules: RuleSet,
    registry: crate::rules::overlay_types::OverlayTypeRegistry,
    bounds: crate::map::playfield::PlayfieldBounds,
    terrain: ResolvedTerrainGrid,
    bridges: BridgeRuntimeState,
    path: PathGrid,
    zones: ZoneGrid,
}

impl FixtureParts {
    fn build(extra: &str, art: &IniFile) -> Self {
        let mut text = String::from(
            "[InfantryTypes]\n0=ENGINEER\n1=JUMPJET\n[JUMPJET]\nStrength=125\nSpeed=9\nSpeedType=Hover\nMovementZone=Fly\nJumpjetSpeed=30\nJumpjetHeight=500\nJumpjetClimb=20\nJumpJet=yes\nBalloonHover=yes\nHoverAttack=yes\nLocomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\n[AircraftTypes]\n0=HORNET\n[HORNET]\nLandable=yes\nSpeed=12\nSpeedType=Winged\nStrength=75\nLocomotor={4A582746-9839-11d1-B709-00A024DDAFD1}\n[BuildingTypes]\n0=CABHUT\n[ENGINEER]\nEngineer=yes\nSpeed=4\nSpeedType=Foot\nStrength=75\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n[CABHUT]\nBridgeRepairHut=yes\nFoundation=1x1\nStrength=200\n[Warheads]\n0=SA\n1=Super\n[Super]\nInfDeath=2\nPenetratesBunker=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[CombatDamage]\nC4Warhead=SA\n[SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[OverlayTypes]\n",
        );
        for id in 0..=238 {
            text.push_str(&format!("{id}=O{id}\n"));
        }
        for id in 0..=238 {
            text.push_str(&format!("[O{id}]\nLand=Clear\nNoUseTileLandType=no\n"));
        }
        for land in crate::rules::terrain_rules::LandType::ALL.iter().take(9) {
            text.push_str(&format!(
                "[{}]\nFoot=100%\nTrack=100%\nWheel=100%\nBuildable=yes\n",
                land.section_name()
            ));
        }
        let mut ini = IniFile::from_str(&text);
        ini.merge(&IniFile::from_str(extra));
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, art).unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(art));
        let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(art));
        let mut terrain = ResolvedTerrainGrid::from_cells(
            33,
            33,
            (0..33)
                .flat_map(|y| {
                    (0..33).map(move |x| {
                        crate::sim::world::lifecycle_tests::common_raw_terrain_cell(x, y, 0, false)
                    })
                })
                .collect(),
        );
        crate::map::resolved_terrain::install_ordinary_repair_test_catalog(&mut terrain);
        terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
        let terrain_rules = crate::rules::terrain_rules::TerrainRules::from_ini(&ini);
        let costs = terrain_rules
            .semantics_for_land_type(0)
            .unwrap()
            .speed_costs;
        for y in 0..33 {
            for x in 0..33 {
                let c = terrain.cell_mut(x, y).unwrap();
                c.speed_costs = costs;
                c.base_speed_costs = costs;
            }
        }
        let bounds =
            crate::map::playfield::PlayfieldBounds::from_normalized_local_size(16, 0, 0, 16, 16);
        let bridges =
            BridgeRuntimeState::from_resolved_terrain_with_map_size(&terrain, true, 300, (16, 16));
        let path = PathGrid::from_resolved_terrain(&terrain);
        let zones = ZoneGrid::build_with_native_map_context(
            &path,
            &terrain,
            bridges.endpoint_records(),
            Some((16, 16)),
            Some(bounds),
        );
        assert!(zones.hierarchy_for(MovementZone::Normal).is_some());
        Self {
            rules,
            registry,
            bounds,
            terrain,
            bridges,
            path,
            zones,
        }
    }
}
