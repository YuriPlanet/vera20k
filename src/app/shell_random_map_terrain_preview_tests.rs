use super::*;

#[test]
fn generated_tree_color_survives_progress_and_thumbnail_persistence() {
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let audio = crate::rules::audio_sources::AudioDefinitions::select(&assets);
    let (_, _, owner) = crate::rules::retail_sources::load_startup_rules(
        &assets,
        std::sync::Arc::clone(audio.sounds()),
        std::sync::Arc::clone(audio.eva()),
    )
    .unwrap()
    .into_parts();
    let options = crate::map::rmg::RmgOptions {
        theater: 0,
        map_type: 3,
        resources: 2,
        ruggedness: 35,
        water_amount: 50,
        num_players: 2,
        tiberium: 35,
        vegetation: 30,
        urban_presence: 40,
        seed: 4242,
        ..Default::default()
    };
    let mode =
        crate::rules::retail_sources::select_mode_ini(&assets, Some("MPBattleMD.ini")).unwrap();
    let prepared =
        prepare_random_map_generation(&mut assets, &options, Some(&owner), mode.as_ref()).unwrap();
    let decision = RandomMapGenerationRetention::default().map_storage_decision(&options);
    let receiver = spawn_random_map_generation_worker(
        options.clone(),
        prepared.settings,
        prepared.resolved_inputs,
        prepared.blocks,
        prepared.tech_types,
        crate::map::resolved_terrain::SharedCellDummy::fresh(),
        decision,
    )
    .unwrap();
    let mut snapshots = Vec::new();
    let generated = loop {
        match receiver
            .recv_timeout(std::time::Duration::from_secs(60))
            .unwrap()
        {
            RandomMapUpdate::Started => {}
            RandomMapUpdate::Progress(snapshot) => snapshots.push(snapshot),
            RandomMapUpdate::Finished(map) => break map,
        }
    };
    let last = snapshots.pop().expect("native95-percent preview");
    assert!(
        snapshots.iter().all(|snapshot| snapshot
            .map_file
            .terrain_objects
            .iter()
            .all(|object| !object.name.starts_with("TREE"))),
        "earlier generation boundaries must not observe later ordinary trees"
    );
    assert!(
        last.map_file
            .terrain_objects
            .iter()
            .any(|object| object.name.starts_with("TREE")),
        "native59930D redraws after the ordinary tree constructors"
    );

    // Expected source is the same production reset/type/asset reader. The
    // existing independently native-tested4BF650 port owns packed arithmetic.
    let processed = owner
        .random_map_preview_projection(mode.as_ref(), "TEMPERATE")
        .unwrap();
    let mut types = crate::rules::ruleset::RuleSet::from_processed_rules(&processed).unwrap();
    types.bind_terrain_assets(
        &assets,
        prepared.theater.extension,
        crate::rules::terrain_asset_catalog::TerrainAssetLoadPhase::Fresh,
    );
    let format = crate::render::native_surface_format::RGB565;
    let mut rng = crate::sim::rng::MainRng::new(0x0401_599d);
    let mut main = crate::app::state::process_main_draws(None, &mut rng);
    let mut selectors = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut selected = None;
    let mut final_colors = Vec::new();
    for map in [&last.map_file, &generated.map_file] {
        let resolved = build_random_map_preview_grid(
            map,
            Some(&prepared.theater),
            Some(&assets),
            Some(&prepared.terrain_rules),
            &mut main,
            &mut selectors,
        );
        let ground = crate::map::rmg::preview::preview_cells_from_map(
            map,
            &resolved,
            &|_, _, _| None,
            &|_, _, _| None,
        );
        let colors = random_map_preview_cells(
            map,
            &resolved,
            prepared.preview_rules.as_ref(),
            Some(&assets),
            prepared.theater.extension,
        );
        let tree = map
            .terrain_objects
            .iter()
            .find(|object| {
                object.name.starts_with("TREE")
                    && selected.is_none_or(|at| at == (object.rx, object.ry))
                    && !map
                        .overlays
                        .iter()
                        .any(|overlay| (overlay.rx, overlay.ry) == (object.rx, object.ry))
                    && ground
                        .iter()
                        .any(|cell| (cell.x, cell.y) == (object.rx, object.ry))
                    && types
                        .terrain_object_type_case_insensitive(&object.name)
                        .is_some_and(|kind| kind.foundation == "1x1" && kind.radar_color != [0; 3])
            })
            .expect("seed4242 produces a visible ordinary tree head without an overlay");
        selected = Some((tree.rx, tree.ry));
        let kind = types
            .terrain_object_type_case_insensitive(&tree.name)
            .unwrap();
        let base = ground
            .iter()
            .find(|cell| (cell.x, cell.y) == (tree.rx, tree.ry))
            .unwrap();
        let colored = colors
            .iter()
            .find(|cell| (cell.x, cell.y) == (tree.rx, tree.ry))
            .unwrap();
        let source = format.pack_rgb8(kind.radar_color);
        let expected = (
            format.unpack_rgb8(format.mix_packed_rgb16(source, format.pack_rgb8(base.left), 75)),
            format.unpack_rgb8(format.mix_packed_rgb16(source, format.pack_rgb8(base.right), 75)),
        );
        assert_ne!(
            (base.left, base.right),
            expected,
            "the selected tree actually changes its thumbnail"
        );
        assert_eq!(
            (colored.left, colored.right),
            expected,
            "Terrain branch6415E1..641689"
        );
        final_colors = colors;
    }
    let placements = &generated.map_file.terrain_objects;
    let first_tree = placements
        .iter()
        .position(|object| object.name.starts_with("TREE"))
        .unwrap();
    let last_spawner = placements
        .iter()
        .rposition(|object| object.name.starts_with("TIBTRE"))
        .unwrap();
    assert!(
        last_spawner < first_tree,
        "native5A2C05 constructors precede5A48AA"
    );
    println!(
        "Generated Terrain preview witness: {selected:?}; spawner: {:?}",
        placements[last_spawner]
    );
    let preview = crate::map::rmg::preview::render_preview(&final_colors, &[]).unwrap();
    let directory = crate::map::source::test_support::TestDirectory::new("terrain-preview");
    let mut modal = Some(crate::ui::skirmish_shell::RandomMapSetupModalState::open(
        options, None, false,
    ));
    modal
        .as_mut()
        .unwrap()
        .finish_generate(Some(preview.clone()));
    dismiss_random_map_setup_owners(
        &mut modal,
        &mut RandomMapGenerationRetention::default(),
        Some(directory.path()),
    );
    let pcx = crate::assets::pcx_file::PcxFile::from_bytes(
        &std::fs::read(directory.path().join(RANDMAP_PREVIEW_FILE)).unwrap(),
    )
    .unwrap();
    let rgb: Vec<_> = preview
        .rgba
        .chunks_exact(4)
        .flat_map(|pixel| pixel[..3].iter().copied())
        .collect();
    assert_eq!(
        pcx.pixels, rgb,
        "tree colors survive common teardown and the chooser's PCX reader"
    );
}
