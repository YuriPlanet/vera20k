use super::test_support::{audio_test_install, make_new_format_mix_bytes};
use super::*;

#[test]
fn inspection_lists_required_failures_and_keeps_later_readable_archives() {
    let directory = audio_test_install("inspect-required");
    for name in ["cachemd.mix", "mapsmd03.mix", "movmd03.mix"] {
        std::fs::remove_file(directory.path().join(name)).unwrap();
    }
    std::fs::write(directory.path().join("cameo.mix"), b"bad").unwrap();

    let (assets, issues) =
        AssetManager::inspect_install(directory.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    // Missing MOVMD03 falls through to MOVIES03 in mount_startup_movie_media;
    // the optional issue names that final attempted fallback archive.
    assert_eq!(
        issues
            .iter()
            .map(|issue| (issue.name.as_str(), issue.required))
            .collect::<Vec<_>>(),
        [
            ("cachemd.mix", true),
            ("cameo.mix", true),
            ("mapsmd03.mix", false),
            ("movies03.mix", false)
        ]
    );
    assert!(issues[1].detail.contains("Invalid MIX archive"));
    assert!(assets.archive("multimd.mix").is_some());
    assert!(matches!(
        AssetManager::new(directory.path(), MediaArchiveMode::STOCK_DIGITAL),
        Err(AssetError::AssetNotFound { name }) if name == "cachemd.mix"
    ));

    // Only the test-only FailFast policy still refuses missing media.
    let mut manager = AssetManager::from_ini_probe_root(directory.path()).unwrap();
    assert!(
        manager
            .mount_native_map_media(MediaArchiveMode::STOCK_DIGITAL)
            .is_err()
    );
    assert!(
        manager
            .mount_native_movie_media(MediaArchiveMode::STOCK_DIGITAL)
            .is_err()
    );
}

#[test]
fn startup_continues_without_map_and_movie_media() {
    let directory = audio_test_install("no-media");
    for name in ["mapsmd03.mix", "multimd.mix", "movmd03.mix"] {
        std::fs::remove_file(directory.path().join(name)).unwrap();
    }
    assert!(AssetManager::new(directory.path(), MediaArchiveMode::STOCK_DIGITAL).is_ok());
    assert!(AssetManager::new(directory.path(), MediaArchiveMode::CdWildcard).is_ok());

    let (_, issues) =
        AssetManager::inspect_install(directory.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    assert_eq!(
        issues
            .iter()
            .map(|issue| (issue.name.as_str(), issue.required))
            .collect::<Vec<_>>(),
        [
            ("mapsmd03.mix", false),
            ("multimd.mix", false),
            ("movies03.mix", false)
        ]
    );
}

#[test]
fn inspection_accepts_nested_required_archives_without_optional_music() {
    let directory = audio_test_install("inspect-nested");
    std::fs::remove_file(directory.path().join("cachemd.mix")).unwrap();
    let nested = make_new_format_mix_bytes("inside.bin", b"nested data");
    directory.write_mix("ra2md.mix", "cachemd.mix", &nested);

    let (assets, issues) =
        AssetManager::inspect_install(directory.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    let source = assets.resolve_ref("inside.bin").unwrap();
    assert_eq!(source.source_archive, "ra2md.mix -> cachemd.mix");
    assert_eq!(source.bytes, b"nested data");
    assert!(assets.archive("thememd.mix").is_none());
    assert!(assets.archive("theme.mix").is_none());
    let ordinary = AssetManager::new(directory.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    assert_eq!(
        assets.loaded_archive_names(),
        ordinary.loaded_archive_names()
    );
}

#[test]
fn inspection_reports_optional_corruption_and_both_cd_media_boundaries() {
    let directory = audio_test_install("inspect-optional");
    std::fs::write(directory.path().join("thememd.mix"), b"bad").unwrap();
    let (_, issues) =
        AssetManager::inspect_install(directory.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].name, "thememd.mix");
    assert!(!issues[0].required);
    let ordinary = AssetManager::new(directory.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    assert!(ordinary.archive("thememd.mix").is_none());

    // Standalone media admission retains its strict error contract even when
    // ordinary startup treats that same optional named archive as a warning.
    std::fs::write(directory.path().join("maps03.mix"), b"bad").unwrap();
    let mut media = AssetManager::from_ini_probe_root(directory.path()).unwrap();
    assert!(
        media
            .mount_native_map_media(MediaArchiveMode::STOCK_DIGITAL)
            .is_err()
    );
    assert!(AssetManager::new(directory.path(), MediaArchiveMode::STOCK_DIGITAL).is_ok());
    std::fs::remove_file(directory.path().join("maps03.mix")).unwrap();

    for name in ["mapsmd03.mix", "movmd03.mix"] {
        std::fs::remove_file(directory.path().join(name)).unwrap();
    }
    let (_, issues) =
        AssetManager::inspect_install(directory.path(), MediaArchiveMode::CdWildcard).unwrap();
    assert_eq!(
        issues
            .iter()
            .map(|issue| (issue.name.as_str(), issue.required))
            .collect::<Vec<_>>(),
        [
            ("MAPSMD*.MIX", false),
            ("thememd.mix", false),
            ("MOVMD*.MIX", false)
        ]
    );
}

/// Startup mounting `ares.mix` (Load_Bootstrap_AresMIX, 0x0053029E) makes
/// Ares play in every game (`rules::ares_switch`).
#[test]
fn a_mounted_ares_mix_ships_ares() {
    let directory = audio_test_install("ships-ares");
    let assets = AssetManager::new(directory.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    assert!(!assets.ships_ares());
    directory.write_mix("ares.mix", "probe.csf", b"ares");
    let assets = AssetManager::new(directory.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    assert!(assets.ships_ares());
}

/// Ares Expand_MIX_Reorg (0x006BD7E3) runs Init_Base_Mix_Files ahead of
/// LANGMD.MIX, and Load_Bootstrap_AresMIX (0x0053029E) registers ares.mix
/// after the expansions and before RA2MD.MIX. A game without `ares.mix` keeps
/// gamemd's order, LANGMD.MIX first.
#[test]
fn a_game_with_ares_mix_resolves_base_mixes_and_ares_mix_ahead_of_langmd() {
    let directory = audio_test_install("ares-mix");
    let source = |name: &str| {
        let assets = AssetManager::new(directory.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
        assets.resolve_ref(name).unwrap().source_archive.to_owned()
    };
    for archive in ["langmd.mix", "ra2md.mix"] {
        directory.write_mix(archive, "probe.csf", archive.as_bytes());
    }
    // A build with the `ares` feature takes Ares' order in every game.
    assert_eq!(
        source("probe.csf"),
        if cfg!(feature = "ares") {
            "ra2md.mix"
        } else {
            "langmd.mix"
        }
    );
    directory.write_mix("ares.mix", "other.csf", b"ares.mix");
    assert_eq!(source("probe.csf"), "ra2md.mix");
    for archive in ["ares.mix", "expandmd00.mix"] {
        directory.write_mix(archive, "probe.csf", archive.as_bytes());
        assert_eq!(source("probe.csf"), archive);
    }
}
