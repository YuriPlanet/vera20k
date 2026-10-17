use super::*;
use crate::map::map_file::MapHeader;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::overlay_grid::{OverlayGrid, WallPointerTarget};
use crate::sim::snapshot::GameSnapshot;

fn stock_tiberium_rules() -> (RuleSet, OverlayTypeRegistry) {
    let mut text = String::from(
        "[Tiberiums]\n0=Riparius\n\
         [Riparius]\nImage=1\nValue=25\nGrowth=2200\nGrowthPercentage=.06\n\
         Spread=2200\nSpreadPercentage=.06\n[OverlayTypes]\n",
    );
    let mut tiberium_names = Vec::new();
    for raw_key in (1..=124).filter(|key| *key != 40 && *key != 41) {
        let name = if (105..=116).contains(&raw_key) {
            format!("TIB{:02}", raw_key - 104)
        } else {
            format!("FILL{raw_key:03}")
        };
        text.push_str(&format!("{raw_key}={name}\n"));
        if name.starts_with("TIB") {
            tiberium_names.push(name);
        }
    }
    for name in tiberium_names {
        text.push_str(&format!("[{name}]\nTiberium=yes\n"));
    }
    let ini = IniFile::from_str(&text);
    (
        RuleSet::from_ini(&ini).expect("tiberium rules"),
        OverlayTypeRegistry::from_ini(&ini, None),
    )
}

#[test]
fn radar_dirty_ack_dedups_one_window_rearms_and_never_changes_hash() {
    let mut sim = Simulation::new();
    let cell = (17, 23);
    let other = (18, 23);
    let baseline_hash = sim.state_hash();

    sim.mark_radar_terrain_dirty_cells([cell, cell]);
    assert_eq!(sim.radar_terrain_dirty_generation, 1);
    assert_eq!(sim.radar_terrain_dirty_cells, vec![cell]);
    sim.mark_radar_terrain_dirty_cells([other, cell, other]);
    let generation = sim.radar_terrain_dirty_generation;
    assert_eq!(generation, 2);
    assert_eq!(sim.radar_terrain_dirty_cells, vec![cell, other]);
    sim.mark_radar_terrain_dirty_cells([cell, other]);
    assert_eq!(sim.radar_terrain_dirty_generation, generation);
    assert_eq!(sim.state_hash(), baseline_hash);

    // A skipped/absent presentation performs no acknowledgement. A stale
    // acknowledgement likewise cannot erase the pending native dirty batch.
    assert!(!sim.acknowledge_radar_terrain_dirty(generation.wrapping_add(1)));
    assert_eq!(sim.radar_terrain_dirty_cells, vec![cell, other]);
    assert!(sim.acknowledge_radar_terrain_dirty(generation));
    assert!(sim.radar_terrain_dirty_cells.is_empty());
    assert_eq!(sim.state_hash(), baseline_hash);

    sim.mark_radar_terrain_dirty_cells([cell]);
    assert_eq!(sim.radar_terrain_dirty_cells, vec![cell]);
    assert_eq!(sim.radar_terrain_dirty_generation, generation + 1);
    assert_eq!(sim.state_hash(), baseline_hash);
}

#[test]
fn radar_dirty_ack_rearms_same_tiberium_cell_across_density_removals() {
    let (rules, registry) = stock_tiberium_rules();
    let tib01 = registry.id_for_name("TIB01").expect("TIB01");
    let cell = (8, 8);
    let mut sim = Simulation::new();
    sim.overlay_grid = Some(OverlayGrid::new(32, 32));

    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(cell.0, cell.1, tib01, 3);
    let _ = sim.overlay_grid.as_mut().unwrap().take_dirty_cells();
    assert!(
        sim.reduce_tiberium_at_with_native_context(cell, 4, Some(&rules), Some(&registry))
            .fully_removed
    );
    assert_eq!(sim.radar_terrain_dirty_cells, vec![cell]);
    assert!(sim.acknowledge_radar_terrain_dirty(1));

    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(cell.0, cell.1, tib01, 5);
    let _ = sim.overlay_grid.as_mut().unwrap().take_dirty_cells();
    assert!(
        sim.reduce_tiberium_at_with_native_context(cell, 6, Some(&rules), Some(&registry))
            .fully_removed
    );
    assert_eq!(sim.radar_terrain_dirty_cells, vec![cell]);
    assert_eq!(sim.radar_terrain_dirty_generation, 2);
}

#[test]
fn radar_dirty_ack_rearms_same_wall_removal() {
    let ini = IniFile::from_str(
        "[OverlayTypes]\n0=GAWALL\n[GAWALL]\nWall=yes\nStrength=100\nDamageLevels=1\n",
    );
    let registry = OverlayTypeRegistry::from_ini(&ini, None);
    let cell = (4, 5);
    let mut sim = Simulation::new();
    sim.overlay_grid = Some(OverlayGrid::new(16, 16));
    sim.resolved_terrain = Some(crate::sim::tiberium::test_support::flat_terrain(16, 16));

    for expected_generation in [13, 26] {
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(cell.0, cell.1, 0, 0);
        let _ = sim.overlay_grid.as_mut().unwrap().take_dirty_cells();
        sim.apply_wall_damage(WallPointerTarget::Real(cell.0, cell.1), -1, &registry);
        assert_eq!(
            sim.radar_terrain_dirty_cells,
            vec![
                (4, 5),
                (4, 3),
                (5, 4),
                (3, 4),
                (4, 4),
                (3, 6),
                (2, 5),
                (3, 5),
                (5, 6),
                (4, 7),
                (4, 6),
                (6, 5),
                (5, 5),
            ],
        );
        assert_eq!(sim.radar_terrain_dirty_generation, expected_generation);
        assert!(sim.acknowledge_radar_terrain_dirty(expected_generation));
    }
}

#[test]
fn radar_dirty_wall_partial_damage_marks_nothing() {
    let ini = IniFile::from_str(
        "[OverlayTypes]\n0=FILL0\n1=FILL1\n2=GAWALL\n[GAWALL]\nWall=yes\nStrength=1\nDamageLevels=4\n",
    );
    let registry = OverlayTypeRegistry::from_ini(&ini, Some(&ini));
    let mut sim = Simulation::new();
    sim.overlay_grid = Some(OverlayGrid::new(16, 16));
    sim.resolved_terrain = Some(crate::sim::tiberium::test_support::flat_terrain(16, 16));
    sim.overlay_grid.as_mut().unwrap().place_overlay(4, 5, 2, 0);
    let _ = sim.overlay_grid.as_mut().unwrap().take_dirty_cells();
    sim.apply_wall_damage(WallPointerTarget::Real(4, 5), 1, &registry);
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(4, 5).overlay_data,
        0x10
    );
    assert!(sim.radar_terrain_dirty_cells.is_empty());
    assert_eq!(sim.radar_terrain_dirty_generation, 0);
    assert_eq!(sim.tactical_dirty_cells, vec![(4, 5)]);
}

#[test]
fn radar_dirty_ack_survives_action40_and_load_rebuild_needs_no_stale_batch() {
    let header = MapHeader {
        theater: "TEMPERATE".to_string(),
        fill: "Clear".to_string(),
        level: 0,
        width: 80,
        height: 58,
        local_left: 2,
        local_top: 4,
        local_width: 76,
        local_height: 48,
    };
    let mut sim = Simulation::new();
    sim.install_playfield_from_map_header(&header);
    sim.mark_radar_terrain_dirty_cells([(20, 20)]);
    assert!(sim.change_visible_map_area([4, 40, 54, 12], None));
    assert_eq!(sim.playfield_revision, 1);
    assert_eq!(sim.radar_terrain_dirty_cells, vec![(20, 20)]);
    assert!(sim.acknowledge_radar_terrain_dirty(1));

    sim.mark_radar_terrain_dirty_cells([(20, 20)]);
    let hash_with_pending_client_state = sim.state_hash();
    let bytes_with_pending = GameSnapshot::save(&sim, 0, 0, "all06umd.map", 0);
    assert!(sim.acknowledge_radar_terrain_dirty(2));
    assert_eq!(sim.state_hash(), hash_with_pending_client_state);
    let bytes_after_ack = GameSnapshot::save(&sim, 0, 0, "all06umd.map", 0);
    assert_eq!(
        bytes_with_pending, bytes_after_ack,
        "client-local pending/ack state is absent from snapshot bytes",
    );
    let restored = GameSnapshot::load(&bytes_with_pending)
        .expect("snapshot loads")
        .sim;
    assert!(restored.radar_terrain_dirty_cells.is_empty());
    assert_eq!(restored.radar_terrain_dirty_generation, 0);
    assert_eq!(restored.playfield_revision, sim.playfield_revision);
}
