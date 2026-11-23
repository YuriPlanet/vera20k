use super::*;
use crate::map::resolved_terrain::test_flat_cell;
use crate::sim::pathfinding::zone_incremental::repair_zone_hierarchy_around_cell;
use crate::util::save_codec;
use std::hash::Hasher;

// Supplied branch-test bounds, as in zone_map_tests::repair_test_bounds. They
// admit the retained flat cells through the native mode1 query; None rejects.
fn bounds() -> crate::map::playfield::PlayfieldBounds {
    crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -1_000,
        off_100: -1_000,
        off_104: 2_000,
        off_108: 2_000,
    }
}

fn fixture() -> (ResolvedTerrainGrid, ZoneGrid) {
    let terrain = ResolvedTerrainGrid::from_cells(
        16,
        16,
        (0..16)
            .flat_map(|y| (0..16).map(move |x| test_flat_cell(x, y)))
            .collect(),
    );
    let path = PathGrid::from_resolved_terrain(&terrain);
    let zones = ZoneGrid::build_with_terrain(&path, &terrain, &[], 16, 16);
    (terrain, zones)
}

fn hash(zones: &ZoneGrid) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    zones.fold_navigation_history(&mut hasher);
    hasher.finish()
}

fn assert_fresh(zones: &ZoneGrid) {
    let cached = zones.navigation_history_hash.get().copied().unwrap();
    assert_eq!(cached, zones.compute_navigation_history_hash());
}

#[test]
fn retained_navigation_fingerprint_invalidates_on_local_patch_and_raw_borrow() {
    let (terrain, mut zones) = fixture();
    let initial = hash(&zones);
    assert_fresh(&zones);
    assert_eq!(initial, hash(&zones));
    let copy = zones.clone();
    assert_eq!(initial, hash(&copy));
    assert_fresh(&copy);
    assert!(zones.refresh_base_cell_attributes_at(&terrain, 4, 4));
    assert!(
        zones.navigation_history_hash.get().is_some(),
        "unchanged Recalc keeps the digest"
    );

    // The production584550 receiver appends new IDs while the source terrain
    // and base rows stay identical, as captured by zone_threat's producer.
    let slots = zones.hierarchy.level(1).unwrap().record_slot_count();
    repair_zone_hierarchy_around_cell(&mut zones, (5, 5), Some(bounds()), &terrain, &[]);
    assert!(zones.hierarchy.level(1).unwrap().record_slot_count() > slots);
    assert!(zones.navigation_history_hash.get().is_none());
    assert_ne!(initial, hash(&zones));
    assert_fresh(&zones);

    let after_patch = hash(&zones);
    zones.base_topology_mut().raw_zone_ids_by_row[0][0] ^= 1;
    assert!(zones.navigation_history_hash.get().is_none());
    assert_ne!(after_patch, hash(&zones));
    assert_fresh(&zones);

    let before_padding = hash(&zones);
    zones.base_and_hierarchy_mut().1.levels_mut()[1].set_native_padding_zone(123, 7);
    assert!(zones.navigation_history_hash.get().is_none());
    assert_ne!(before_padding, hash(&zones));
    assert_fresh(&zones);
}

#[test]
fn fingerprint_tracks_original_edge_order_and_native_threat_seed() {
    use super::super::zone_hierarchy::{ZoneEdgeRecord, ZoneRecord};
    let (_, zones) = fixture();
    let mut variant = zones.clone();
    let mut hierarchy = variant.hierarchy.clone();
    let graph = &mut hierarchy.levels_mut()[1];
    let zone = graph.zone_at(4, 4);
    let record = graph.record(zone).unwrap();
    let seed = if record.native_threat_index() == Some(131) {
        (4, 0)
    } else {
        (0, 0)
    };
    let initial = hash(&variant);
    graph.set_record(ZoneRecord::from_seed(
        zone,
        record.parent,
        record.zone_type,
        seed,
        crate::map::cell_index::CellGrid::Retail,
    ));
    variant.replace_hierarchy(hierarchy);
    assert_ne!(initial, hash(&variant));
    assert_fresh(&variant);

    let mut graph = zones.hierarchy.clone();
    graph.levels_mut()[0].push_edge(1, ZoneEdgeRecord::new(2, 0));
    graph.levels_mut()[0].push_edge(1, ZoneEdgeRecord::new(3, 1));
    let mut reversed = graph.clone();
    reversed.levels_mut()[0].clear_edges(1);
    reversed.levels_mut()[0].push_edge(1, ZoneEdgeRecord::new(3, 1));
    reversed.levels_mut()[0].push_edge(1, ZoneEdgeRecord::new(2, 0));
    let mut a = zones.clone();
    let mut b = zones.clone();
    a.replace_hierarchy(graph);
    b.replace_hierarchy(reversed);
    assert_ne!(hash(&a), hash(&b));
    assert_fresh(&a);
    assert_fresh(&b);
}

#[test]
fn public_dimensions_are_folded_outside_the_cached_navigation_fingerprint() {
    let (_, mut zones) = fixture();
    let initial = hash(&zones);
    let cached = zones.navigation_history_hash.get().copied();
    zones.width += 1;
    assert_ne!(initial, hash(&zones));
    assert_eq!(cached, zones.navigation_history_hash.get().copied());
    assert_fresh(&zones);
}

#[test]
fn ordinary_load_retains_saved_base_rows_and_heights_instead_of_recomputing_them() {
    let (terrain, mut live) = fixture();
    // Native Mouse Save copies these retained bytes, even when a current
    // Cell/PathGrid projection differs. It does not call RebuildConnectivity.
    let base = live.base_topology_mut();
    base.levels[5 * 16 + 5] = 3;
    base.raw_zone_ids_by_row[0][0] = 17;
    base.raw_zone_ids_by_row[12][0] = 0xffff;
    let bytes = save_codec::serialize(&live).unwrap();
    let mut restored: ZoneGrid = save_codec::deserialize(&bytes).unwrap();
    assert!(restored.is_native_load_pending());
    assert!(restored.navigation_history_hash.get().is_none());
    assert_eq!(save_codec::serialize(&restored).unwrap(), bytes);
    let path = PathGrid::from_resolved_terrain(&terrain);
    restored
        .finish_native_load(&path, &terrain, &[], None)
        .unwrap();
    assert!(!restored.is_native_load_pending());
    assert_eq!(save_codec::serialize(&restored).unwrap(), bytes);
    assert_eq!(restored.base_topology.levels[5 * 16 + 5], 3);
    assert_eq!(restored.base_topology.raw_zone_ids_by_row[0][0], 17);
    assert_eq!(restored.base_topology.raw_zone_ids_by_row[12][0], 0xffff);
    assert_ne!(bytes, save_codec::serialize(&fixture().1).unwrap());
}

#[test]
fn malformed_saved_navigation_planes_and_rows_are_rejected_before_publication() {
    let (_, mut live) = fixture();
    live.base_topology_mut().levels.pop();
    let bytes = save_codec::serialize(&live).unwrap();
    assert!(save_codec::deserialize::<ZoneGrid>(&bytes).is_err());
    let (_, mut live) = fixture();
    live.base_topology_mut().raw_zone_ids_by_row[12].pop();
    let bytes = save_codec::serialize(&live).unwrap();
    assert!(save_codec::deserialize::<ZoneGrid>(&bytes).is_err());
    let (_, live) = fixture();
    let mut restored: ZoneGrid =
        save_codec::deserialize(&save_codec::serialize(&live).unwrap()).unwrap();
    let small = ResolvedTerrainGrid::from_cells(1, 1, vec![test_flat_cell(0, 0)]);
    assert!(
        restored
            .finish_native_load(&PathGrid::from_resolved_terrain(&small), &small, &[], None)
            .is_err()
    );
    assert!(restored.is_native_load_pending());
}
