use super::*;
use crate::map::map_file::MapHeader;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::overlay_grid::{OverlayGrid, WallPointerTarget};
use crate::sim::snapshot::GameSnapshot;

#[test]
fn radar_dirty_ack_dedups_one_window_rearms_and_never_changes_hash() {
    let mut sim = Simulation::new();
    let cell = (17, 23);
    let other = (18, 23);
    let baseline_hash = sim.state_hash();

    sim.dirty_cells.mark_radar([cell, cell]);
    assert_eq!(sim.dirty_cells.radar_generation(), 1);
    assert_eq!(sim.dirty_cells.radar(), vec![cell]);
    sim.dirty_cells.mark_radar([other, cell, other]);
    let generation = sim.dirty_cells.radar_generation();
    assert_eq!(generation, 2);
    assert_eq!(sim.dirty_cells.radar(), vec![cell, other]);
    sim.dirty_cells.mark_radar([cell, other]);
    assert_eq!(sim.dirty_cells.radar_generation(), generation);
    assert_eq!(sim.state_hash(), baseline_hash);

    // A skipped/absent presentation performs no acknowledgement. A stale
    // acknowledgement likewise cannot erase the pending native dirty batch.
    assert!(
        !sim.dirty_cells
            .acknowledge_radar(generation.wrapping_add(1))
    );
    assert_eq!(sim.dirty_cells.radar(), vec![cell, other]);
    assert!(sim.dirty_cells.acknowledge_radar(generation));
    assert!(sim.dirty_cells.radar().is_empty());
    assert_eq!(sim.state_hash(), baseline_hash);

    sim.dirty_cells.mark_radar([cell]);
    assert_eq!(sim.dirty_cells.radar(), vec![cell]);
    assert_eq!(sim.dirty_cells.radar_generation(), generation + 1);
    assert_eq!(sim.state_hash(), baseline_hash);
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
    assert!(sim.dirty_cells.radar().is_empty());
    assert_eq!(sim.dirty_cells.radar_generation(), 0);
    assert_eq!(sim.dirty_cells.tactical_for_test(), vec![(4, 5)]);
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
    sim.dirty_cells.mark_radar([(20, 20)]);
    assert!(sim.change_visible_map_area(
        [4, 40, 54, 12],
        crate::rules::ruleset::RuleSet::empty_for_test()
    ));
    assert_eq!(sim.playfield_revision, 1);
    assert_eq!(sim.dirty_cells.radar(), vec![(20, 20)]);
    assert!(sim.dirty_cells.acknowledge_radar(1));

    sim.dirty_cells.mark_radar([(20, 20)]);
    let hash_with_pending_client_state = sim.state_hash();
    let bytes_with_pending = GameSnapshot::save(&sim, 0, 0, "all06umd.map", 0);
    assert!(sim.dirty_cells.acknowledge_radar(2));
    assert_eq!(sim.state_hash(), hash_with_pending_client_state);
    let bytes_after_ack = GameSnapshot::save(&sim, 0, 0, "all06umd.map", 0);
    assert_eq!(
        bytes_with_pending, bytes_after_ack,
        "client-local pending/ack state is absent from snapshot bytes",
    );
    let restored = GameSnapshot::load(&bytes_with_pending)
        .expect("snapshot loads")
        .sim;
    assert!(restored.dirty_cells.radar().is_empty());
    assert_eq!(restored.dirty_cells.radar_generation(), 0);
    assert_eq!(restored.playfield_revision, sim.playfield_revision);
}
