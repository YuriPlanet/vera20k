use super::*;

fn native_corpus() -> serde_json::Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_pavement.json",
    ))
    .unwrap()
}

#[test]
fn pavement_resident_pristine_gate_matches_all_native_unsigned_subtiles() {
    let corpus = native_corpus();
    let predicate = corpus["control"][0]["predicate"].as_array().unwrap();
    let mut bytes = vec![0u8; 16 + 4 * predicate.len() + 52 * predicate.len()];
    bytes[0..4].copy_from_slice(&(predicate.len() as u32).to_le_bytes());
    bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&60u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&30u32.to_le_bytes());
    for (sub, damaged) in predicate.iter().enumerate() {
        let Some(damaged) = damaged.as_bool() else {
            continue;
        };
        let offset = 16 + 4 * predicate.len() + 52 * sub;
        bytes[16 + 4 * sub..20 + 4 * sub].copy_from_slice(&(offset as u32).to_le_bytes());
        bytes[offset + 36..offset + 40].copy_from_slice(&(u32::from(damaged) << 2).to_le_bytes());
    }
    let mut grid = ResolvedTerrainGrid::from_cells(0, 0, Vec::new());
    grid.native_tmp_draw_heights.insert(
        100,
        PristineTmpHeader {
            subtiles: TmpFile::pristine_header_fields_from_bytes(&bytes).unwrap(),
            total_file_count: 2,
        },
    );
    for sub in 0..=u8::MAX {
        assert_eq!(
            grid.native_tmp_has_damaged_data(100, sub),
            corpus["control"][0]["gate_sweep"][usize::from(sub)].as_bool(),
            "sub={sub}"
        );
        assert_eq!(grid.native_tmp_draw_height(100, sub), 30);
    }
    assert_eq!(
        grid.native_tmp_has_damaged_data(101, 0),
        None,
        "uncached is not a sparse false"
    );
    assert_eq!(grid.native_tmp_has_damaged_data(-1, 0), None);
    assert_eq!(grid.native_tmp_file_count(100), Some(2));
}

#[test]
#[ignore = "requires active retail assets and VERA20K_XMP34U4_MAP"]
fn retail_pavement_resident_gate_survives_normal_loader_asset_drop() {
    let retail = std::env::var("RA2_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            crate::util::config::GameConfig::load()
                .unwrap()
                .paths
                .ra2_dir
        });
    let map = std::env::var("VERA20K_XMP34U4_MAP").unwrap_or_else(|_| "xmp34u4.map".into());
    let scenario = crate::headless_scenario::load(&retail, &map, 0x0B21_D6E5).unwrap();
    let sim = scenario.sim();
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let corpus = native_corpus();
    // This plain road receiver is the case skipped by the old BRS-only writer.
    let cell = terrain.cell(66, 102).unwrap();
    assert_eq!((cell.final_tile_index, cell.final_sub_tile), (278, 7));
    assert!(!cell.has_bridge_deck && !cell.bridge_facts.has_structural_bridge());
    for sub in 0..=u8::MAX {
        assert_eq!(
            terrain.native_tmp_has_damaged_data(278, sub),
            corpus["stock"]["gate_sweep"][usize::from(sub)].as_bool(),
            "stocksub={sub}"
        );
    }
    println!(
        "256 stock pristine damaged-data queries match original5471F0 after normal loader assets drop"
    );
}
