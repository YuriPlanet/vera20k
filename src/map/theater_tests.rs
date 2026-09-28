//! Tests for theater INI parsing and tileset lookups.
//!
//! Extracted from theater.rs to stay under the 400-line limit.

use super::*;

fn make_test_ini() -> String {
    "[TileSet0000]\nSetName=Grass\nFileName=clear\nTilesInSet=1\n\n\
     [TileSet0001]\nSetName=Blank\nFileName=\nTilesInSet=1\n\n\
     [TileSet0002]\nSetName=Cliffs\nFileName=cliff\nTilesInSet=3\n"
        .to_string()
}

#[test]
fn active_theater_names_and_archive_order_match_retail() {
    let expected = [
        (
            "TEMPERATE",
            "temperatmd.ini",
            &["temperat.mix", "tem.mix", "isotemmd.mix", "isotemp.mix"][..],
        ),
        (
            "SNOW",
            "snowmd.ini",
            &[
                "snowmd.mix",
                "snow.mix",
                "sno.mix",
                "isosnomd.mix",
                "isosnow.mix",
            ][..],
        ),
        (
            "URBAN",
            "urbanmd.ini",
            &["urban.mix", "urb.mix", "isourbmd.mix", "isourb.mix"][..],
        ),
        (
            "DESERT",
            "desertmd.ini",
            &["desert.mix", "des.mix", "isodesmd.mix", "isodes.mix"][..],
        ),
        (
            "NEWURBAN",
            "urbannmd.ini",
            &["urbann.mix", "ubn.mix", "isoubnmd.mix", "isoubn.mix"][..],
        ),
        (
            "LUNAR",
            "lunarmd.ini",
            &["lunar.mix", "lun.mix", "isolunmd.mix", "isolun.mix"][..],
        ),
    ];

    for (name, ini_name, archives) in expected {
        let def = theater_def(name).expect("active theater definition");
        assert_eq!(def.ini_name, ini_name);
        assert_eq!(def.mix_archives, archives);
    }
}

#[test]
fn missing_theater_palette_uses_native_rgb_ramp() {
    let palette = native_missing_theater_palette();
    assert_eq!(
        palette.colors[0],
        Color {
            r: 0,
            g: 255,
            b: 0,
            a: 0,
        }
    );
    assert_eq!(palette.colors[1], Color::rgb(1, 254, 4));
    assert_eq!(palette.colors[64], Color::rgb(64, 191, 0));
    assert_eq!(palette.colors[255], Color::rgb(255, 0, 252));
}

#[test]
fn test_parse_tileset_ini_basic() {
    let ini: &str = &make_test_ini();
    let lookup: TilesetLookup = parse_tileset_ini(ini.as_bytes(), "tem").expect("Should parse");

    assert_eq!(lookup.len(), 5); // 1 + 1 + 3
    assert_eq!(lookup.filename(0), Some("clear01.tem"));
    assert_eq!(lookup.filename(1), None); // blank
    assert_eq!(lookup.filename(2), Some("cliff01.tem"));
    assert_eq!(lookup.filename(4), Some("cliff03.tem"));

    // Tileset boundary/index lookups.
    assert_eq!(lookup.bounds().len(), 3);
    assert_eq!(lookup.tileset_index(0), Some(0)); // tile 0 → TileSet0000
    assert_eq!(lookup.tileset_index(1), Some(1)); // tile 1 → TileSet0001
    assert_eq!(lookup.tileset_index(2), Some(2)); // tile 2 → TileSet0002
    assert_eq!(lookup.tileset_index(4), Some(2)); // tile 4 → TileSet0002
    assert_eq!(lookup.tileset_index(99), None); // out of range

    // Edge cases: NO_TILE, negative, and far out-of-range.
    assert_eq!(lookup.filename(NO_TILE), None);
    assert_eq!(lookup.filename(-1), None);
    assert_eq!(lookup.filename(999), None);
}

#[test]
fn gsi_04_02_last_tiles_parser_builds_native_ordered_exceptions_and_boundaries() {
    let lookup = parse_tileset_ini(
        b"[TileSet0000]\nTilesInSet=3\nFileName=a\n\n\
          [TileSet0001]\nTilesInSet=5\nLastTilesInSet=2\nFileName=b\n\n\
          [TileSet0002]\nTilesInSet=4\nLastTilesInSet=6\nFileName=c\n",
        "tem",
    )
    .expect("verified compatibility example parses");

    assert_eq!(
        lookup.legacy_tile_index_exceptions,
        vec![
            LegacyTileIndexException {
                legacy_boundary: 5,
                delta: 3,
            },
            LegacyTileIndexException {
                legacy_boundary: 11,
                delta: -2,
            },
        ]
    );
    assert_eq!(lookup.len(), 12);
    assert_eq!(lookup.translate_legacy_map_tile_index(4), 4);
    assert_eq!(lookup.translate_legacy_map_tile_index(5), 8);
    assert_eq!(lookup.translate_legacy_map_tile_index(10), 13);
    assert_eq!(lookup.translate_legacy_map_tile_index(11), 12);
    assert_eq!(
        lookup.translate_legacy_map_tile_index(i32::from(u16::MAX)),
        i32::from(u16::MAX),
        "only positive 0xFFFF is the native no-tile sentinel"
    );
}

#[test]
fn gsi_04_02_last_tiles_transform_is_signed_original_ordered_and_wrapping() {
    let signed = parse_tileset_ini(b"[TileSet0000]\nTilesInSet=2\nLastTilesInSet=-2\n", "tem")
        .expect("negative-boundary theater");
    assert_eq!(signed.translate_legacy_map_tile_index(-1), 3);
    assert_eq!(signed.translate_legacy_map_tile_index(0), 4);
    assert_eq!(
        signed.translate_legacy_map_tile_index(i32::from(u16::MAX)),
        i32::from(u16::MAX),
    );

    let original_raw = parse_tileset_ini(
        b"[TileSet0000]\nTilesInSet=105\nLastTilesInSet=5\n\n\
          [TileSet0001]\nTilesInSet=1\nLastTilesInSet=45\n",
        "tem",
    )
    .expect("original-input comparison theater");
    assert_eq!(original_raw.translate_legacy_map_tile_index(5), 105);

    let nonmonotonic = parse_tileset_ini(
        b"[TileSet0000]\nTilesInSet=11\nLastTilesInSet=10\n\n\
          [TileSet0001]\nTilesInSet=-4\nLastTilesInSet=-5\n",
        "tem",
    )
    .expect("nonmonotonic declaration-order theater");
    assert_eq!(
        nonmonotonic.legacy_tile_index_exceptions[1].legacy_boundary,
        5
    );
    assert_eq!(nonmonotonic.translate_legacy_map_tile_index(6), 6);

    let wraps_max = parse_tileset_ini(
        format!(
            "[TileSet0000]\nTilesInSet={}\nLastTilesInSet={}\n",
            i32::MIN,
            i32::MAX
        )
        .as_bytes(),
        "tem",
    )
    .expect("wrapping delta theater");
    assert_eq!(
        wraps_max.legacy_tile_index_exceptions,
        vec![LegacyTileIndexException {
            legacy_boundary: i32::MAX,
            delta: 1,
        }]
    );
    assert_eq!(
        wraps_max.translate_legacy_map_tile_index(i32::MAX),
        i32::MIN
    );

    let mut wraps_min = parse_tileset_ini(b"", "tem").expect("empty theater");
    wraps_min.legacy_tile_index_exceptions = vec![LegacyTileIndexException {
        legacy_boundary: i32::MIN,
        delta: -1,
    }];
    assert_eq!(
        wraps_min.translate_legacy_map_tile_index(i32::MIN),
        i32::MAX
    );
}

#[test]
fn gsi_04_02_empty_last_tiles_table_is_full_signed_identity() {
    let lookup = parse_tileset_ini(b"[TileSet0000]\nTilesInSet=1\n", "tem")
        .expect("empty compatibility table");
    assert!(lookup.legacy_tile_index_exceptions.is_empty());
    for raw in [i32::MIN, -123, -1, 0, 123, i32::from(u16::MAX), i32::MAX] {
        assert_eq!(lookup.translate_legacy_map_tile_index(raw), raw);
    }
}

#[test]
fn gsi_04_02_last_tiles_parser_preserves_readint_termination_and_wrapping() {
    let parsed = parse_tileset_ini(
        b"[TileSet0000]\nTilesInSet=junk\nLastTilesInSet=-1\n\n\
          [TileSet0001]\nTilesInSet=$3\nLastTilesInSet=1H\n\n\
          [TileSet0002]\nTilesInSet=-1\n\n\
          [TileSet0003]\nTilesInSet=9\nLastTilesInSet=0\n",
        "tem",
    )
    .expect("ReadInt edge theater");
    assert_eq!(parsed.bounds().len(), 2);
    assert_eq!(parsed.bounds()[0].count, 0, "present junk is atoi zero");
    assert_eq!(
        parsed.bounds()[1].count,
        3,
        "native hexadecimal is accepted"
    );
    assert_eq!(
        parsed.legacy_tile_index_exceptions,
        vec![LegacyTileIndexException {
            legacy_boundary: 1,
            delta: 2,
        }]
    );

    for terminator in [
        "[TileSet0000]\nTilesInSet=0\n\n\
         [TileSet0001]\nFileName=missing-key\n\n\
         [TileSet0002]\nTilesInSet=9\n",
        "[TileSet0000]\nTilesInSet=0\n\n\
         [TileSet0001]\nTilesInSet=\n\n\
         [TileSet0002]\nTilesInSet=9\n",
    ] {
        let terminated = parse_tileset_ini(terminator.as_bytes(), "tem")
            .expect("missing or empty TilesInSet terminates");
        assert_eq!(terminated.bounds().len(), 1);
        assert_eq!(terminated.len(), 0);
    }

    let malformed_last = parse_tileset_ini(
        b"[TileSet0000]\nTilesInSet=2\nLastTilesInSet=not-a-number\n",
        "tem",
    )
    .expect("present malformed LastTilesInSet is native atoi zero");
    assert_eq!(
        malformed_last.legacy_tile_index_exceptions,
        vec![LegacyTileIndexException {
            legacy_boundary: 0,
            delta: 2,
        }]
    );

    let suppressed = parse_tileset_ini(
        b"[TileSet0000]\nTilesInSet=2\nLastTilesInSet=-1\n\n\
          [TileSet0001]\nTilesInSet=3\nLastTilesInSet=3\n",
        "tem",
    )
    .expect("suppressed records");
    assert!(suppressed.legacy_tile_index_exceptions.is_empty());

    let negative = parse_tileset_ini(
        b"[TileSet0000]\nTilesInSet=-2\n\n\
          [TileSet0001]\nTilesInSet=2\nLastTilesInSet=0\n",
        "tem",
    )
    .expect("nonterminating negative count");
    assert_eq!(negative.bounds()[0], TilesetBounds { start: 0, count: 0 });
    assert_eq!(negative.bounds()[1], TilesetBounds { start: 0, count: 2 });
    assert_eq!(
        negative.legacy_tile_index_exceptions,
        vec![LegacyTileIndexException {
            legacy_boundary: -2,
            delta: 2,
        }]
    );

    let cursor_wrap = parse_tileset_ini(
        format!(
            "[TileSet0000]\nTilesInSet=0\nLastTilesInSet={}\n\n\
             [TileSet0001]\nTilesInSet=0\nLastTilesInSet=1\n",
            i32::MAX
        )
        .as_bytes(),
        "tem",
    )
    .expect("wrapping legacy cursor");
    assert_eq!(
        cursor_wrap.legacy_tile_index_exceptions,
        vec![
            LegacyTileIndexException {
                legacy_boundary: i32::MAX,
                delta: -i32::MAX,
            },
            LegacyTileIndexException {
                legacy_boundary: i32::MIN,
                delta: -1,
            },
        ]
    );
}

#[test]
fn gsi_04_02_last_tiles_safe_domains_and_native_string_defaults() {
    let maximum = parse_tileset_ini(b"[TileSet0000]\nTilesInSet=65535\n", "tem")
        .expect("all usable u16 tile IDs fit");
    assert_eq!(maximum.len(), 65_535);
    assert_eq!(maximum.bounds()[0].count, u16::MAX);
    assert_eq!(maximum.set_name(0), Some("No Name"));
    assert_eq!(maximum.filename(0), None);

    assert!(matches!(
        parse_tileset_ini(b"[TileSet0000]\nTilesInSet=65536\n", "tem"),
        Err(crate::map::map_file::MapError::TilesetRegistryTooLarge {
            attempted: 65_536,
            maximum: 65_535,
        })
    ));
    assert!(matches!(checked_tileset_ordinal(65_535), Ok(65_535)));
    assert!(matches!(
        checked_tileset_ordinal(65_536),
        Err(crate::map::map_file::MapError::TilesetOrdinalOverflow {
            ordinal: 65_536,
            maximum: 65_535,
        })
    ));

    let absent = IniFile::from_str("[TileSet65535]\nTilesInSet=0\n");
    assert!(
        read_tileset_row(&absent, 65_536)
            .expect("absent row retains the native -1 terminator")
            .is_none()
    );
    let nonterminating = IniFile::from_str("[TileSet65536]\nTilesInSet=0\n");
    assert!(matches!(
        read_tileset_row(&nonterminating, 65_536),
        Err(crate::map::map_file::MapError::TilesetOrdinalOverflow {
            ordinal: 65_536,
            maximum: 65_535,
        })
    ));
}

#[test]
fn gsi_04_02_tileset_10000_is_not_an_artificial_terminator() {
    let mut ini = String::new();
    for ordinal in 0..10_000u32 {
        ini.push_str(&format!("[TileSet{ordinal:04}]\nTilesInSet=0\n\n"));
    }
    ini.push_str("[TileSet10000]\nTilesInSet=1\nFileName=late\n");
    let lookup = parse_tileset_ini(ini.as_bytes(), "tem").expect("ordinal 10000 parses");
    assert_eq!(lookup.bounds().len(), 10_001);
    assert_eq!(
        lookup.bounds()[10_000],
        TilesetBounds { start: 0, count: 1 }
    );
    assert_eq!(lookup.filename(0), Some("late01.tem"));
}

#[test]
#[ignore = "requires VERA20K_RETAIL_THEATER_INI_DIR"]
fn gsi_04_02_all_retail_theaters_have_empty_compatibility_tables() {
    let root = std::path::PathBuf::from(
        std::env::var_os("VERA20K_RETAIL_THEATER_INI_DIR")
            .expect("set VERA20K_RETAIL_THEATER_INI_DIR to extracted retail ini directory"),
    );
    let expected = [
        ("temperatmd.ini", 82, 838, 837),
        ("snowmd.ini", 83, 964, 960),
        ("urbanmd.ini", 111, 1_081, 1_077),
        ("urbannmd.ini", 122, 1_175, 1_174),
        ("desertmd.ini", 82, 726, 726),
        ("lunarmd.ini", 85, 198, 192),
    ];
    for (name, section_count, slot_count, last_start) in expected {
        let bytes = std::fs::read(root.join(name)).expect("read extracted retail theater INI");
        let lookup = parse_tileset_ini(&bytes, "tmp").expect("parse retail theater INI");
        assert!(lookup.legacy_tile_index_exceptions.is_empty(), "{name}");
        assert_eq!(lookup.bounds().len(), section_count, "{name}");
        assert_eq!(lookup.len(), slot_count, "{name}");
        assert_eq!(
            lookup.bounds().last().map(|bounds| bounds.start),
            Some(last_start),
            "{name}"
        );
    }
}

#[test]
fn gsi_02_11_actual_chain_count_stops_at_first_missing_sibling() {
    let present = [
        "clear01.urb",
        "clear01a.urb",
        "clear01b.urb",
        "clear01c.urb",
        "clear01d.urb",
        "clear01e.urb",
        "clear01f.urb",
        "clear01g.urb",
        // A later file must not bridge the missing `h` slot.
        "clear01i.urb",
    ];
    let siblings = contiguous_variant_filenames("clear01.urb", |name| present.contains(&name));
    assert_eq!(siblings.len(), 7);
    assert_eq!(siblings.first().map(String::as_str), Some("clear01a.urb"));
    assert_eq!(siblings.last().map(String::as_str), Some("clear01g.urb"));

    let orphaned = ["clear01a.urb", "clear01b.urb"];
    assert!(
        contiguous_variant_filenames("clear01.urb", |name| orphaned.contains(&name)).is_empty()
    );
}

#[test]
fn gsi_02_11_file_index_resolves_the_exact_independent_tmp_owner() {
    let mut lookup = parse_tileset_ini(
        b"[TileSet0000]\nSetName=Clear\nFileName=clear\nTilesInSet=1\n",
        "urb",
    )
    .expect("synthetic tileset");
    lookup.variant_filenames[0] = vec!["clear01a.urb".to_string(), "clear01b.urb".to_string()];

    assert_eq!(lookup.filename_for_variant(0, 0), Some("clear01.urb"));
    assert_eq!(lookup.filename_for_variant(0, 1), Some("clear01a.urb"));
    assert_eq!(lookup.filename_for_variant(0, 2), Some("clear01b.urb"));
    assert_eq!(lookup.filename_for_variant(0, 3), None);
}

#[test]
fn gsi_02_11_positive_subtile_wrap_preserves_requested_identity_boundary() {
    assert_eq!(wrapped_subtile_index(0, 2, 3), Some(0));
    assert_eq!(wrapped_subtile_index(9, 2, 3), Some(3));
    assert_eq!(wrapped_subtile_index(u8::MAX, 2, 3), Some(3));
    assert_eq!(wrapped_subtile_index(7, 0, 3), None);
}

#[test]
fn test_collect_used_tiles() {
    let cells: Vec<(i32, u8)> = vec![(0, 0), (1, 0), (0, 0), (NO_TILE, 0), (2, 1)];
    let used: HashSet<TileKey> = collect_used_tiles(&cells);
    assert_eq!(used.len(), 3); // (0,0), (1,0), (2,1) — deduped, NO_TILE excluded.
    assert!(used.contains(&TileKey {
        tile_id: 0,
        sub_tile: 0,
        variant: 0,
    }));
    assert!(used.contains(&TileKey {
        tile_id: 2,
        sub_tile: 1,
        variant: 0,
    }));
}

#[test]
fn test_theater_def_lookup() {
    assert!(theater_def("TEMPERATE").is_some());
    assert!(theater_def("temperate").is_some());
    assert!(theater_def("SNOW").is_some());
    assert!(theater_def("URBAN").is_some());
    assert!(theater_def("DESERT").is_some());
    assert!(theater_def("LUNAR").is_some());
    assert!(theater_def("NEWURBAN").is_some());
    assert!(theater_def("BOGUS").is_none());
}

#[test]
fn test_is_water_and_cliff() {
    let ini_str: &str = "\
[TileSet0000]\nSetName=Grass\nFileName=clear\nTilesInSet=2\n\n\
[TileSet0001]\nSetName=Water\nFileName=water\nTilesInSet=3\n\n\
[TileSet0002]\nSetName=Water Cliffs\nFileName=wcliff\nTilesInSet=2\n\n\
[TileSet0003]\nSetName=Cliffs\nFileName=cliff\nTilesInSet=1\n";
    let lookup: TilesetLookup = parse_tileset_ini(ini_str.as_bytes(), "tem").expect("Should parse");

    // Grass (tile_ids 0-1): not water, not cliff.
    assert!(!lookup.is_water(0));
    assert!(!lookup.is_cliff(0));
    // Water (tile_ids 2-4): water but not cliff.
    assert!(lookup.is_water(2));
    assert!(lookup.is_water(4));
    assert!(!lookup.is_cliff(2));
    // Water Cliffs (tile_ids 5-6): both water and cliff.
    assert!(lookup.is_water(5));
    assert!(lookup.is_cliff(5));
    // Cliffs (tile_id 7): cliff but not water.
    assert!(!lookup.is_water(7));
    assert!(lookup.is_cliff(7));
    // Out of range: neither.
    assert!(!lookup.is_water(99));
    assert!(!lookup.is_cliff(99));
}

#[test]
fn parses_morphable_flag_per_tileset() {
    let ini = b"[TileSet0000]\n\
                FileName=foo\n\
                TilesInSet=1\n\
                SetName=Foo\n\
                Morphable=yes\n\
                \n\
                [TileSet0001]\n\
                FileName=bar\n\
                TilesInSet=1\n\
                SetName=Bar\n\
                \n\
                [TileSet0002]\n\
                TilesInSet=-1\n";
    let lookup = parse_tileset_ini(ini, "tem").unwrap();
    // tile_id 0 = first tile of TileSet0000 (Morphable=yes)
    assert!(lookup.is_morphable(0));
    // tile_id 1 = first tile of TileSet0001 (Morphable= unset → default false)
    assert!(!lookup.is_morphable(1));
}

#[test]
fn theater_parse_allow_tiberium_defaults_false() {
    let ini = b"[TileSet0000]\n\
                FileName=clear\n\
                TilesInSet=2\n\
                SetName=Clear\n\
                \n\
                [TileSet0001]\n\
                FileName=tiber\n\
                TilesInSet=2\n\
                SetName=Tiberium\n\
                AllowTiberium=true\n\
                \n\
                [TileSet0002]\n\
                FileName=rough\n\
                TilesInSet=1\n\
                SetName=Rough\n\
                AllowTiberium=false\n";
    let lookup = parse_tileset_ini(ini, "tem").unwrap();

    assert!(!lookup.allows_tiberium(0));
    assert!(!lookup.allows_tiberium(1));
    assert!(lookup.allows_tiberium(2));
    assert!(lookup.allows_tiberium(3));
    assert!(!lookup.allows_tiberium(4));
    assert!(!lookup.allows_tiberium(99));
}

#[test]
fn general_reader_finds_bridge_middle_keys() {
    let ini = "[General]\nBridgeSet=5\nBridgeMiddle1=7\nBridgeMiddle2=12\n\n[TileSet0000]\nTilesInSet=1\nFileName=clear\n";
    let parsed = IniFile::from_bytes(ini.as_bytes()).unwrap();
    let general = parsed.section("General");
    assert_eq!(super::read_general_u16(general, "BridgeMiddle1"), Some(7));
    assert_eq!(super::read_general_u16(general, "BridgeMiddle2"), Some(12));
}

#[test]
fn general_reader_missing_bridge_middle_returns_none() {
    let ini = "[General]\nBridgeSet=5\n\n[TileSet0000]\nTilesInSet=1\nFileName=clear\n";
    let parsed = IniFile::from_bytes(ini.as_bytes()).unwrap();
    let general = parsed.section("General");
    assert_eq!(super::read_general_u16(general, "BridgeMiddle1"), None);
    assert_eq!(super::read_general_u16(general, "BridgeMiddle2"), None);
}

fn bridge_piece_values(keys: super::TheaterBridgePieceKeys) -> [Option<u16>; 10] {
    [
        keys.bridge_top_left_1,
        keys.bridge_top_left_2,
        keys.bridge_bottom_right_1,
        keys.bridge_bottom_right_2,
        keys.bridge_top_right_1,
        keys.bridge_top_right_2,
        keys.bridge_bottom_left_1,
        keys.bridge_bottom_left_2,
        keys.bridge_middle_1,
        keys.bridge_middle_2,
    ]
}

fn loaded_bridge_piece_values(theater: &TheaterData) -> [Option<u16>; 10] {
    [
        theater.bridge_top_left_1,
        theater.bridge_top_left_2,
        theater.bridge_bottom_right_1,
        theater.bridge_bottom_right_2,
        theater.bridge_top_right_1,
        theater.bridge_top_right_2,
        theater.bridge_bottom_left_1,
        theater.bridge_bottom_left_2,
        theater.bridge_middle_1,
        theater.bridge_middle_2,
    ]
}

#[test]
fn bridge_piece_parser_keeps_all_ten_keys_independent() {
    let ini = "[General]\n\
BridgeTopLeft1=11\n\
BridgeTopLeft2=12\n\
BridgeBottomRight1=13\n\
BridgeBottomRight2=14\n\
BridgeTopRight1=15\n\
BridgeTopRight2=16\n\
BridgeBottomLeft1=17\n\
BridgeBottomLeft2=18\n\
BridgeMiddle1=19\n\
BridgeMiddle2=20\n";

    let parsed = IniFile::from_bytes(ini.as_bytes()).unwrap();
    let general = parsed.section("General");
    assert_eq!(
        bridge_piece_values(super::read_bridge_piece_keys(general)),
        [
            Some(11),
            Some(12),
            Some(13),
            Some(14),
            Some(15),
            Some(16),
            Some(17),
            Some(18),
            Some(19),
            Some(20),
        ]
    );
}

#[test]
fn bridge_piece_parser_preserves_native_absent_values() {
    let ini = "[General]\n\
BridgeTopLeft1=-1\n\
BridgeBottomRight1=3\n\
BridgeBottomRight2=-1\n\
BridgeBottomLeft1=6\n\
BridgeMiddle1=256\n\
BridgeMiddle2=70000\n";

    let parsed = IniFile::from_bytes(ini.as_bytes()).unwrap();
    let general = parsed.section("General");
    assert_eq!(
        bridge_piece_values(super::read_bridge_piece_keys(general)),
        [
            None,
            None,
            Some(3),
            None,
            None,
            None,
            Some(6),
            None,
            Some(256),
            None,
        ]
    );
}

fn native_general_corpus() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../tools/rules_oracle/theater_general_reader.json"
    ))
    .expect("executed 545150 General reader corpus")
}

#[test]
fn selected_retail_temperate_rmg_roles_reach_tile_ids() {
    use crate::map::rmg::tiles::{SpecialTerrain, TileIds};
    use crate::util::sha256::sha256_hex;

    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let theater = load_theater(&mut assets, "TEMPERATE").expect("load retail temperate theater");
    let corpus = native_general_corpus();
    let physical = corpus["physical"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["file"] == "temperatmd.ini")
        .expect("checked native physical temperate input");
    assert_eq!(
        sha256_hex(&theater.ini_data),
        physical["sha256"].as_str().unwrap()
    );

    // Reader/default/ordinal correctness is independently compared with native
    // execution by general_helpers_match_original_reader_and_signed_repair_keys
    // and general_roles_match_original_ordinal_publication below. This assertion
    // covers production loading/publication, without another 18-key parser.
    let ini = IniFile::from_bytes(&theater.ini_data).expect("selected theater INI");
    let expected = resolve_rmg_tile_keys(&theater.lookup, ini.section("General"));
    assert_eq!(
        theater.rmg_tiles, expected,
        "loader publishes every RMG role"
    );
    let mut loaded_ids = TileIds::resolve(&theater);
    // SpecialTerrain comes from cliff_ranges, not these 18 RMG keys. Its native
    // predicates have separate tests; this is only the RMG projection boundary.
    loaded_ids.special = SpecialTerrain::default();
    assert_eq!(loaded_ids, TileIds::from_keys(&expected));
}

#[test]
fn general_helpers_match_original_reader_and_signed_repair_keys() {
    use crate::map::bridge_rim_tiles::HighBridgeRimTiles;

    let corpus = native_general_corpus();
    for case in corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(corpus["physical"].as_array().unwrap())
    {
        let name = case["name"]
            .as_str()
            .or_else(|| case["file"].as_str())
            .unwrap();
        let text = case["input_text"]
            .as_str()
            .or_else(|| case["general_text"].as_str())
            .unwrap();
        let ini = IniFile::from_bytes(text.as_bytes()).unwrap();
        let general = ini.section("General");
        for read in corpus["read_contract"].as_array().unwrap() {
            let key = read["key"].as_str().unwrap();
            let default = i32::try_from(read["default"].as_i64().unwrap()).unwrap();
            assert_eq!(
                i64::from(general.map_or(default, |section| section.read_int(key, default))),
                case["values"][key].as_i64().unwrap(),
                "{name}: original General/{key} at {}",
                read["call"]
            );
        }
        for key in [
            "BridgeSet",
            "WoodBridgeSet",
            "SlopeSetPieces",
            "SlopeSetPieces2",
            "Tunnels",
            "TrackTunnels",
            "DirtTunnels",
            "DirtTrackTunnels",
        ] {
            assert_eq!(
                read_general_u16(general, key),
                u16::try_from(case["values"][key].as_i64().unwrap()).ok(),
                "{name}: bounded {key} ordinal"
            );
        }
        let rim = HighBridgeRimTiles::from_ini(0, text.as_bytes());
        let signed_pieces = [
            rim.top_left,
            rim.bottom_right,
            rim.top_right,
            rim.bottom_left,
            rim.middle,
        ]
        .concat();
        let projected = bridge_piece_values(read_bridge_piece_keys(general));
        // Compare the two consumer representations with the same original
        // signed DWORDs; oversized/negative repair identities must survive.
        for (index, key) in [
            "BridgeTopLeft1",
            "BridgeTopLeft2",
            "BridgeBottomRight1",
            "BridgeBottomRight2",
            "BridgeTopRight1",
            "BridgeTopRight2",
            "BridgeBottomLeft1",
            "BridgeBottomLeft2",
            "BridgeMiddle1",
            "BridgeMiddle2",
        ]
        .iter()
        .enumerate()
        {
            let native = case["values"][key].as_i64().unwrap();
            assert_eq!(
                i64::from(signed_pieces[index]),
                native,
                "{name}: signed {key}"
            );
            assert_eq!(
                projected[index],
                u16::try_from(native).ok(),
                "{name}: projected {key}"
            );
        }
    }
}

#[test]
fn general_roles_match_original_ordinal_publication() {
    let corpus = native_general_corpus();
    for case in corpus["projection_cases"].as_array().unwrap() {
        let native = &case["ordinal_projection"];
        let ordinal = native["supplied_ordinal"].as_u64().unwrap() as usize;
        let Ok(base) = u16::try_from(native["supplied_cumulative_base"].as_i64().unwrap()) else {
            // The corpus also records native DWORD widths beyond the loader's
            // checked u16 capacity; those are not representable tile fixtures.
            continue;
        };
        let ini = IniFile::from_bytes(case["input_text"].as_str().unwrap().as_bytes()).unwrap();
        let general = ini.section("General");
        let mut lookup = parse_tileset_ini(b"", "tem").unwrap();
        lookup.tileset_bounds = vec![TilesetBounds { start: 0, count: 0 }; ordinal + 1];
        lookup.tileset_bounds[ordinal].start = base;
        let cliffs = resolve_cliff_ranges(
            &lookup,
            general,
            read_general_u16(general, "BridgeSet"),
            read_general_u16(general, "WoodBridgeSet"),
        );
        let rmg = resolve_rmg_tile_keys(&lookup, general);
        for (key, actual) in [
            ("BridgeSet", cliffs.bridge_set),
            ("WoodBridgeSet", cliffs.wood_bridge_set),
            ("CliffSet", cliffs.cliff_set),
            ("CliffRamps", cliffs.cliff_ramps),
            ("WaterCliffs", cliffs.water_cliffs),
            ("DestroyableCliffs", cliffs.destroyable_cliffs),
            ("WaterCaves", cliffs.water_caves),
            ("WaterfallEast", cliffs.waterfall_east),
            ("WaterfallWest", cliffs.waterfall_west),
            ("WaterfallNorth", cliffs.waterfall_north),
            ("WaterfallSouth", cliffs.waterfall_south),
            ("ClearTile", rmg.clear_tile),
            ("RampBase", rmg.ramp_base),
            ("RampSmooth", rmg.ramp_smooth),
            ("RoughTile", rmg.rough_tile),
            ("SandTile", rmg.sand_tile),
            ("GreenTile", rmg.green_tile),
            ("ClearToRoughLat", rmg.clear_to_rough_lat),
            ("ClearToSandLat", rmg.clear_to_sand_lat),
            ("ClearToGreenLat", rmg.clear_to_green_lat),
            ("ClearToPaveLat", rmg.clear_to_pave_lat),
            ("PaveTile", rmg.pave_tile),
            ("WaterSet", rmg.water_set),
            ("ShorePieces", rmg.shore_pieces),
            ("WaterBridge", rmg.water_bridge),
            ("MiscPaveTile", rmg.misc_pave_tile),
            ("PavedRoads", rmg.paved_roads),
            ("PavedRoadEnds", rmg.paved_road_ends),
            ("Medians", rmg.medians),
        ] {
            let read_ordinal = case["values"][key].as_i64().unwrap();
            if read_ordinal >= 0 && read_ordinal < ordinal as i64 {
                // Each native control executes one loop ordinal, not earlier
                // rows. Those other rows have no supplied cumulative bases.
                continue;
            }
            assert_eq!(
                actual,
                u16::try_from(native["values"][key].as_i64().unwrap()).ok(),
                "{}: {key}",
                case["name"]
            );
        }
    }
}

#[test]
#[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
fn active_retail_automatic_shell_corpus_is_exact() {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
    use crate::assets::tmp_file::TmpFile;

    let retail_dir = std::env::var_os("RA2_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            crate::util::config::GameConfig::load()
                .ok()
                .map(|config| config.paths.ra2_dir)
        })
        .expect("set RA2_DIR to the installed retail RA2/YR directory");
    assert!(retail_dir.is_dir(), "{}", retail_dir.display());
    let mut assets = AssetManager::new(&retail_dir, MediaArchiveMode::STOCK_DIGITAL)
        .expect("load retail MIX stack");

    let mut loaded_assets = 0usize;
    let mut candidate_base_ids = 0usize;
    let mut present_subcells = 0usize;
    let mut positives = BTreeSet::new();
    let mut missing_assets = BTreeSet::new();
    let mut variant_assets = BTreeSet::new();
    let corpus = native_general_corpus();
    for theater_name in ["TEMPERATE", "SNOW", "URBAN", "NEWURBAN", "DESERT", "LUNAR"] {
        let theater = load_theater(&mut assets, theater_name)
            .unwrap_or_else(|| panic!("load active retail theater {theater_name}"));
        let expected = corpus["physical"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["file"].as_str() == Some(theater_def(theater_name).unwrap().ini_name))
            .unwrap();
        let mut digest = crate::util::sha256::Sha256::new();
        digest.update(&theater.ini_data);
        assert_eq!(
            crate::util::sha256::digest_hex(digest.finalize()),
            expected["sha256"].as_str().unwrap(),
            "{theater_name}: executed physical INI"
        );
        let expected_bridge_pieces = [
            "BridgeTopLeft1",
            "BridgeTopLeft2",
            "BridgeBottomRight1",
            "BridgeBottomRight2",
            "BridgeTopRight1",
            "BridgeTopRight2",
            "BridgeBottomLeft1",
            "BridgeBottomLeft2",
            "BridgeMiddle1",
            "BridgeMiddle2",
        ]
        .map(|key| u16::try_from(expected["values"][key].as_i64().unwrap()).ok());
        assert_eq!(
            loaded_bridge_piece_values(&theater),
            expected_bridge_pieces,
            "loaded {theater_name} bridge-piece keys"
        );

        let mut candidate_tiles = BTreeSet::new();
        for set_index in [
            theater.tunnels,
            theater.track_tunnels,
            theater.dirt_tunnels,
            theater.dirt_track_tunnels,
        ] {
            let Some(bounds) =
                set_index.and_then(|index| theater.lookup.bounds().get(index as usize))
            else {
                continue;
            };
            for offset in 0..4u16 {
                let Some(tile_id) = bounds.start.checked_add(offset) else {
                    continue;
                };
                if usize::from(tile_id) < theater.lookup.len() {
                    candidate_tiles.insert(tile_id);
                }
            }
        }

        candidate_base_ids += candidate_tiles.len();
        for tile_id in candidate_tiles {
            for variant in 0..theater.lookup.total_file_count(tile_id) {
                let filename = theater
                    .lookup
                    .filename_for_variant(tile_id, variant)
                    .expect("enumerated TMP variant has a filename");
                if variant != 0 {
                    variant_assets.insert(format!("{theater_name}/{filename}"));
                }
                let Some(bytes) = assets.get(filename) else {
                    missing_assets.insert(format!("{theater_name}/{filename}"));
                    continue;
                };
                let tmp = TmpFile::from_bytes(&bytes)
                    .unwrap_or_else(|error| panic!("parse {theater_name}/{filename}: {error}"));
                loaded_assets += 1;
                for (subtile, tile) in tmp.tiles.iter().enumerate() {
                    let Some(tile) = tile else {
                        continue;
                    };
                    present_subcells += 1;
                    if tile.terrain_type == 5 {
                        positives.insert(format!("{theater_name}/{filename}/{subtile}"));
                    }
                }
            }
        }
    }

    let mut expected = BTreeSet::new();
    for (theater, filename, subtiles) in [
        ("URBAN", "tunnel01.urb", &[3, 6, 9][..]),
        ("URBAN", "tunnel02.urb", &[1, 2, 3][..]),
        ("URBAN", "tunnel03.urb", &[3, 6, 9][..]),
        ("URBAN", "tunnel04.urb", &[1, 2, 3][..]),
        ("URBAN", "dtunn01.urb", &[3, 6, 9][..]),
        ("URBAN", "dtunn02.urb", &[1, 2, 3][..]),
        ("URBAN", "dtunn03.urb", &[3, 6, 9][..]),
        ("URBAN", "dtunn04.urb", &[1, 2, 3][..]),
        ("NEWURBAN", "tunnel01.ubn", &[3, 6, 9][..]),
        ("NEWURBAN", "tunnel02.ubn", &[1, 2, 3][..]),
        ("NEWURBAN", "tunnel03.ubn", &[3, 6, 9][..]),
        ("NEWURBAN", "tunnel04.ubn", &[1, 2, 3][..]),
    ] {
        for subtile in subtiles {
            expected.insert(format!("{theater}/{filename}/{subtile}"));
        }
    }

    let expected_missing = [
        "NEWURBAN/MDrodc01.ubn",
        "NEWURBAN/MDrodc02.ubn",
        "SNOW/MDrodc01.sno",
        "SNOW/MDrodc02.sno",
        "SNOW/MDrodc03.sno",
        "SNOW/MDrodc04.sno",
        "URBAN/MDrodc01.urb",
        "URBAN/MDrodc02.urb",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(missing_assets, expected_missing);

    let expected_variants = [
        "LUNAR/Green01a.lun",
        "LUNAR/Green01b.lun",
        "LUNAR/Green01c.lun",
        "LUNAR/Green01d.lun",
        "LUNAR/Green01e.lun",
        "LUNAR/Green01f.lun",
        "LUNAR/Green01g.lun",
        "LUNAR/glat01a.lun",
        "LUNAR/glat02a.lun",
        "LUNAR/glat03a.lun",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(variant_assets, expected_variants);
    assert_eq!(
        (
            candidate_base_ids,
            loaded_assets,
            present_subcells,
            positives.len()
        ),
        (64, 66, 731, 36),
        "positive set: {positives:#?}"
    );
    assert_eq!(positives, expected);
}

#[test]
fn cliff_ranges_resolve_ordinals_to_cumulative_tile_starts() {
    let ini = b"[TileSet0000]\nTilesInSet=2\nFileName=clear\nSetName=Clear\n\n\
                [TileSet0001]\nTilesInSet=3\nFileName=shore\nSetName=Shore\n\n\
                [TileSet0002]\nTilesInSet=4\nFileName=cliff\nSetName=Anything\n";
    let lookup = super::parse_tileset_ini(ini, "tem").unwrap();

    assert_eq!(super::resolve_tileset_start(&lookup, 0), Some(0));
    assert_eq!(super::resolve_tileset_start(&lookup, 1), Some(2));
    assert_eq!(super::resolve_tileset_start(&lookup, 2), Some(5));
    assert_eq!(super::resolve_tileset_start(&lookup, -1), None);
    assert_eq!(super::resolve_tileset_start(&lookup, 99), None);
}

#[test]
fn gsi_04_03a_rmg_tile_keys_parse_ramp_smooth_and_resolve_ordinals() {
    let ini_bytes =
        b"[General]\nClearTile = 0\nRampBase = 1\nRampSmooth = 2\nGreenTile = 2\nWaterSet = 1\n\n\
                [TileSet0000]\nTilesInSet=2\nFileName=clear\nSetName=Clear\n\n\
                [TileSet0001]\nTilesInSet=3\nFileName=water\nSetName=Water\n\n\
                [TileSet0002]\nTilesInSet=4\nFileName=green\nSetName=Green\n";
    let lookup = super::parse_tileset_ini(ini_bytes, "tem").unwrap();
    let ini = IniFile::from_bytes(ini_bytes).unwrap();
    let keys = super::resolve_rmg_tile_keys(&lookup, ini.section("General"));

    assert_eq!(keys.clear_tile, Some(0));
    assert_eq!(keys.ramp_base, Some(2));
    assert_eq!(keys.ramp_smooth, Some(5));
    assert_eq!(keys.water_set, Some(2), "set 1 starts at cumulative tile 2");
    assert_eq!(
        keys.green_tile,
        Some(5),
        "set 2 starts at cumulative tile 5"
    );
    assert_eq!(keys.sand_tile, None, "absent key must stay None, not 0");
    assert_eq!(keys.shore_pieces, None);
}

#[test]
fn gsi_04_03a_special_terrain_ranges_match_half_open_boundaries() {
    let ranges = TheaterCliffRanges {
        cliff_set: Some(100),
        cliff_ramps: Some(200),
        water_cliffs: Some(300),
        destroyable_cliffs: Some(400),
        water_caves: Some(500),
        ..TheaterCliffRanges::default()
    };

    assert!(ranges.is_special_terrain_tile(100, 0));
    assert!(ranges.is_special_terrain_tile(139, 0));
    assert!(!ranges.is_special_terrain_tile(140, 0));
    assert!(ranges.is_special_terrain_tile(219, 0));
    assert!(!ranges.is_special_terrain_tile(220, 0));
    assert!(ranges.is_special_terrain_tile(327, 0));
    assert!(!ranges.is_special_terrain_tile(328, 0));
    assert!(ranges.is_special_terrain_tile(401, 0));
    assert!(!ranges.is_special_terrain_tile(402, 0));
    assert!(ranges.is_special_terrain_tile(503, 0));
    assert!(!ranges.is_special_terrain_tile(504, 0));
}

#[test]
fn gsi_04_03a_waterfall_endpoints_use_iso_subtile_not_slope() {
    let ranges = TheaterCliffRanges {
        waterfall_east: Some(10),
        waterfall_west: Some(20),
        waterfall_south: Some(30),
        waterfall_north: Some(40),
        ..TheaterCliffRanges::default()
    };

    assert!(!ranges.is_special_terrain_tile(10, 0));
    assert!(!ranges.is_special_terrain_tile(10, 4));
    assert!(ranges.is_special_terrain_tile(10, 1));
    assert!(ranges.is_special_terrain_tile(11, 0));
    assert!(ranges.is_special_terrain_tile(12, 4));
    assert!(!ranges.is_special_terrain_tile(23, 3));
    assert!(ranges.is_special_terrain_tile(23, 2));
    assert!(!ranges.is_special_terrain_tile(30, 1));
    assert!(ranges.is_special_terrain_tile(30, 2));
    assert!(!ranges.is_special_terrain_tile(43, 2));
    assert!(ranges.is_special_terrain_tile(43, 1));
}

#[test]
fn gsi_04_03a_bridge_ramp_predicate_remains_narrower_than_special_terrain() {
    let ranges = TheaterCliffRanges {
        cliff_set: Some(100),
        cliff_ramps: Some(200),
        water_cliffs: Some(300),
        destroyable_cliffs: Some(400),
        water_caves: Some(500),
        waterfall_east: Some(600),
        bridge_set: Some(700),
        wood_bridge_set: Some(800),
        ..TheaterCliffRanges::default()
    };

    assert!(ranges.is_special_terrain_tile(700, 0));
    assert!(ranges.is_special_terrain_tile(715, 0));
    assert!(!ranges.is_special_terrain_tile(716, 0));
    assert!(ranges.is_special_terrain_tile(800, 0));
    assert!(ranges.is_special_terrain_tile(815, 0));
    assert!(!ranges.is_special_terrain_tile(816, 0));
    assert!(ranges.is_on_bridge_ramp_tile(100, 0));
    assert!(ranges.is_on_bridge_ramp_tile(200, 0));
    assert!(ranges.is_on_bridge_ramp_tile(601, 0));
    assert!(!ranges.is_on_bridge_ramp_tile(600, 0));
    assert!(!ranges.is_on_bridge_ramp_tile(300, 0));
    assert!(!ranges.is_on_bridge_ramp_tile(400, 0));
    assert!(!ranges.is_on_bridge_ramp_tile(500, 0));
    assert!(!ranges.is_on_bridge_ramp_tile(700, 0));
    assert!(!ranges.is_on_bridge_ramp_tile(800, 0));
}

#[test]
fn gsi_04_03a_lunar_theater_zeroing_clears_special_terrain_globals() {
    let mut ini = String::from(
        "[General]\n\
         CliffSet=10\n\
         WaterCliffs=15\n\
         CliffRamps=25\n\
         BridgeSet=19\n\
         WoodBridgeSet=80\n\
         WaterfallEast=49\n\n",
    );
    for idx in 0..=80 {
        let tiles = if idx == 0 { 1 } else { 0 };
        ini.push_str(&format!(
            "[TileSet{idx:04}]\nTilesInSet={tiles}\nFileName=set{idx}\nSetName=Set{idx}\n\n"
        ));
    }
    let lookup = super::parse_tileset_ini(ini.as_bytes(), "lun").unwrap();
    let parsed = IniFile::from_bytes(ini.as_bytes()).unwrap();
    let general = parsed.section("General");
    let mut bridge_set = super::read_general_u16(general, "BridgeSet");
    let mut wood_bridge_set = super::read_general_u16(general, "WoodBridgeSet");
    let mut ranges = super::resolve_cliff_ranges(&lookup, general, bridge_set, wood_bridge_set);
    let mut rmg_tiles = RmgTileKeys {
        water_set: Some(17),
        ..RmgTileKeys::default()
    };

    assert!(ranges.is_special_terrain_tile(1, 0));

    super::apply_lunar_global_zeroing(
        "LUNAR",
        &mut bridge_set,
        &mut wood_bridge_set,
        &mut ranges,
        &mut rmg_tiles,
    );

    assert_eq!(bridge_set, None);
    assert_eq!(wood_bridge_set, None);
    assert_eq!(ranges, TheaterCliffRanges::default());
    assert_eq!(rmg_tiles.water_set, None);
    assert!(!ranges.is_special_terrain_tile(1, 0));
}

#[test]
fn bridge_railing_slope_starts_use_tileset_bounds() {
    let ini = b"[TileSet0000]\nTilesInSet=2\nFileName=clear\nSetName=Clear\n\n\
                [TileSet0001]\nTilesInSet=3\nFileName=slopea\nSetName=Slope A\n\n\
                [TileSet0002]\nTilesInSet=4\nFileName=slopeb\nSetName=Slope B\n";
    let lookup = super::parse_tileset_ini(ini, "tem").unwrap();
    let empty_palette = crate::assets::pal_file::Palette::from_bytes(&[0u8; 768])
        .expect("768-byte zero palette parses");
    let td = super::TheaterData {
        lookup,
        iso_palette: empty_palette.clone(),
        unit_palette: empty_palette.clone(),
        tiberium_palette: empty_palette,
        extension: "tem",
        ini_data: Vec::new(),
        bridge_set: None,
        wood_bridge_set: None,
        slope_set_pieces: Some(1),
        slope_set_pieces2: Some(2),
        bridge_top_left_1: None,
        bridge_top_left_2: None,
        bridge_bottom_right_1: None,
        bridge_bottom_right_2: None,
        bridge_top_right_1: None,
        bridge_top_right_2: None,
        bridge_bottom_left_1: None,
        bridge_bottom_left_2: None,
        bridge_middle_1: None,
        bridge_middle_2: None,
        tunnels: None,
        track_tunnels: None,
        dirt_tunnels: None,
        dirt_track_tunnels: None,
        automatic_tube_bases: [-1; 4],
        cliff_ranges: TheaterCliffRanges::default(),
        rmg_tiles: super::RmgTileKeys::default(),
    };

    assert_eq!(td.bridge_railing_slope_starts(), Some((2, 5)));
}

/// Helper: build a minimal TheaterData for variant-table tests. BridgeSet
/// at tileset index 0 with 20 tiles starting at tile_id 0. Palettes are
/// all-zero (tests never read pixels).
fn synthetic_theater_with_bridge_keys(
    bridge_middle_1: Option<u16>,
    bridge_middle_2: Option<u16>,
) -> super::TheaterData {
    let ini = b"[TileSet0000]\nTilesInSet=20\nFileName=bridge\nSetName=Bridge\n";
    let lookup = super::parse_tileset_ini(ini, "tem").unwrap();
    let empty_palette = crate::assets::pal_file::Palette::from_bytes(&[0u8; 768])
        .expect("768-byte zero palette parses");
    super::TheaterData {
        lookup,
        iso_palette: empty_palette.clone(),
        unit_palette: empty_palette.clone(),
        tiberium_palette: empty_palette,
        extension: "tem",
        ini_data: Vec::new(),
        bridge_set: Some(0),
        wood_bridge_set: None,
        slope_set_pieces: None,
        slope_set_pieces2: None,
        bridge_top_left_1: Some(1),
        bridge_top_left_2: Some(2),
        bridge_bottom_right_1: Some(3),
        bridge_bottom_right_2: Some(3),
        bridge_top_right_1: Some(4),
        bridge_top_right_2: Some(5),
        bridge_bottom_left_1: Some(6),
        bridge_bottom_left_2: Some(6),
        bridge_middle_1,
        bridge_middle_2,
        tunnels: None,
        track_tunnels: None,
        dirt_tunnels: None,
        dirt_track_tunnels: None,
        automatic_tube_bases: [-1; 4],
        cliff_ranges: TheaterCliffRanges::default(),
        rmg_tiles: super::RmgTileKeys::default(),
    }
}

#[test]
fn ramp_tile_table_matches_binary_height_predicates() {
    use crate::map::bridge_facts::BridgeRampKind;

    let td = synthetic_theater_with_bridge_keys(Some(7), Some(12));
    let table = BridgeRampTileTable::from_theater(&td).expect("ramp table");

    assert_eq!(
        table.match_relative_tile(4, 0x0C).map(|r| r.kind),
        Some(BridgeRampKind::TopRight)
    );
    assert_eq!(table.match_relative_tile(4, 0x08).map(|r| r.kind), None);
    assert_eq!(
        table.match_relative_tile(1, 0x08).map(|r| r.kind),
        Some(BridgeRampKind::TopLeft)
    );
    assert_eq!(
        table.match_relative_tile(7, 0x04).map(|r| r.kind),
        Some(BridgeRampKind::Middle1)
    );
    assert_eq!(
        table.match_relative_tile(10, 0x04).map(|r| r.kind),
        Some(BridgeRampKind::Middle1)
    );
    assert_eq!(table.match_relative_tile(11, 0x04), None);
    assert_eq!(
        table.match_relative_tile(12, 0x02).map(|r| r.kind),
        Some(BridgeRampKind::Middle2)
    );
}

#[test]
fn bottom_piece_keys_do_not_enter_the_ramp_classifier() {
    let td = synthetic_theater_with_bridge_keys(Some(7), Some(12));
    assert_eq!(td.bridge_bottom_right_1, Some(3));
    assert_eq!(td.bridge_bottom_right_2, Some(3));
    assert_eq!(td.bridge_bottom_left_1, Some(6));
    assert_eq!(td.bridge_bottom_left_2, Some(6));

    let table = BridgeRampTileTable::from_theater(&td).expect("ramp table");
    for (relative_tile, height) in [(3, 0x0c), (3, 0x08), (6, 0x0c), (6, 0x08)] {
        assert_eq!(
            table.match_relative_tile(relative_tile, height),
            None,
            "bottom pavement key {relative_tile} at height {height:#x}"
        );
    }
}

#[test]
fn ramp_tile_match_tile_id_uses_one_based_bridge_key() {
    use crate::map::bridge_facts::BridgeRampKind;

    let td = synthetic_theater_with_bridge_keys(Some(7), Some(12));
    let table = BridgeRampTileTable::from_theater(&td).expect("ramp table");

    assert_eq!(
        table.match_tile_id(103, 100, 20, 0x0C).map(|r| r.kind),
        Some(BridgeRampKind::TopRight)
    );
    assert_eq!(
        table
            .match_tile_id(103, 100, 20, 0x0C)
            .map(|r| r.relative_tile_index),
        Some(4)
    );
}

#[test]
fn ramp_tile_match_tile_id_rejects_tile_before_bridge_set() {
    let td = synthetic_theater_with_bridge_keys(Some(7), Some(12));
    let table = BridgeRampTileTable::from_theater(&td).expect("ramp table");

    assert_eq!(table.match_tile_id(99, 100, 20, 0x0C), None);
}

#[test]
fn ramp_tile_match_tile_id_rejects_tile_at_bridge_set_end() {
    let table = BridgeRampTileTable {
        top_right_1: Some(4),
        top_right_2: None,
        top_left_1: None,
        top_left_2: None,
        middle_1: None,
        middle_2: None,
    };

    assert_eq!(table.match_tile_id(120, 100, 20, 0x0C), None);
    assert_eq!(table.match_tile_id(104, 100, 20, 0x0C), None);
}

#[test]
fn variant_table_temperate_values() {
    use super::BridgeAnchorVariantTable;
    let td = synthetic_theater_with_bridge_keys(Some(7), Some(12));
    let table = BridgeAnchorVariantTable::from_theater(&td).expect("table");
    // BridgeSet starts at tile_id 0 (TilesInSet=20, first tileset). NS
    // variants: BS + M1 + {-1, 0, 1, 2} = {6, 7, 8, 9}. EW: {11, 12, 13, 14}.
    assert_eq!(table.ns, [6, 7, 8, 9]);
    assert_eq!(table.ew, [11, 12, 13, 14]);
}

#[test]
fn variant_table_returns_none_on_missing_middle_1() {
    use super::BridgeAnchorVariantTable;
    let td = synthetic_theater_with_bridge_keys(None, Some(12));
    assert!(BridgeAnchorVariantTable::from_theater(&td).is_none());
}

#[test]
fn variant_table_returns_none_on_missing_middle_2() {
    use super::BridgeAnchorVariantTable;
    let td = synthetic_theater_with_bridge_keys(Some(7), None);
    assert!(BridgeAnchorVariantTable::from_theater(&td).is_none());
}

#[test]
fn variant_table_returns_none_on_zero_middle() {
    use super::BridgeAnchorVariantTable;
    let td = synthetic_theater_with_bridge_keys(Some(0), Some(12));
    assert!(BridgeAnchorVariantTable::from_theater(&td).is_none());
}

#[test]
fn variant_table_returns_none_on_out_of_bounds() {
    use super::BridgeAnchorVariantTable;
    // TilesInSet=20 → max tile_id 19. BridgeMiddle1=18 → 4th variant
    // = 0+18-1+3 = 20 (OOB).
    let td = synthetic_theater_with_bridge_keys(Some(18), Some(12));
    assert!(BridgeAnchorVariantTable::from_theater(&td).is_none());
}

#[test]
fn tile_id_for_variant0_returns_none() {
    use super::BridgeAnchorVariantTable;
    use crate::sim::bridge_state::{Axis, BridgeheadAnchorClass};
    let td = synthetic_theater_with_bridge_keys(Some(7), Some(12));
    let table = BridgeAnchorVariantTable::from_theater(&td).unwrap();
    assert_eq!(
        table.tile_id_for(Axis::NS, BridgeheadAnchorClass::Variant0),
        None
    );
    assert_eq!(
        table.tile_id_for(Axis::EW, BridgeheadAnchorClass::Variant0),
        None
    );
}

#[test]
fn tile_id_for_each_class_per_axis() {
    use super::BridgeAnchorVariantTable;
    use crate::sim::bridge_state::{Axis, BridgeheadAnchorClass};
    let td = synthetic_theater_with_bridge_keys(Some(7), Some(12));
    let table = BridgeAnchorVariantTable::from_theater(&td).unwrap();
    assert_eq!(
        table.tile_id_for(Axis::NS, BridgeheadAnchorClass::Variant1),
        Some(7)
    );
    assert_eq!(
        table.tile_id_for(Axis::NS, BridgeheadAnchorClass::Damaged),
        Some(8)
    );
    assert_eq!(
        table.tile_id_for(Axis::NS, BridgeheadAnchorClass::AboutToFall),
        Some(9)
    );
    assert_eq!(
        table.tile_id_for(Axis::EW, BridgeheadAnchorClass::AboutToFall),
        Some(14)
    );
}

#[test]
fn match_tile_id_round_trip_all_variants() {
    use super::BridgeAnchorVariantTable;
    use crate::sim::bridge_state::{Axis, BridgeheadAnchorClass};
    let td = synthetic_theater_with_bridge_keys(Some(7), Some(12));
    let table = BridgeAnchorVariantTable::from_theater(&td).unwrap();
    const CLASS_ORDER: [BridgeheadAnchorClass; 4] = [
        BridgeheadAnchorClass::Variant0,
        BridgeheadAnchorClass::Variant1,
        BridgeheadAnchorClass::Damaged,
        BridgeheadAnchorClass::AboutToFall,
    ];
    for (axis, expected_arr) in [(Axis::NS, &table.ns), (Axis::EW, &table.ew)] {
        for (slot, &tid) in expected_arr.iter().enumerate() {
            let (got_axis, got_class) = table.match_tile_id(tid).expect("matched");
            assert_eq!(got_axis, axis);
            assert_eq!(got_class, CLASS_ORDER[slot]);
        }
    }
}

#[test]
fn match_tile_id_rejects_non_variant() {
    use super::BridgeAnchorVariantTable;
    let td = synthetic_theater_with_bridge_keys(Some(7), Some(12));
    let table = BridgeAnchorVariantTable::from_theater(&td).unwrap();
    // BS+5 (one before Variant0 NS), BS+10 (between NS and EW), BS+15
    // (post-AboutToFall EW), 999 (outside BridgeSet).
    assert_eq!(table.match_tile_id(5), None);
    assert_eq!(table.match_tile_id(10), None);
    assert_eq!(table.match_tile_id(15), None);
    assert_eq!(table.match_tile_id(999), None);
}

/// Retail shape: the animation keys sit in the section named by `SetName`, not
/// in `[TileSetNNNN]`. Values copied from `ini/temperatmd.ini` — `[TileSet0053]
/// SetName=Tunnel Floor` / `[Tunnel Floor]`, and `[Waterfalls-C]`.
fn make_tile_anim_ini() -> String {
    "[TileSet0000]\nSetName=Grass\nFileName=clear\nTilesInSet=1\n\n\
     [TileSet0001]\nSetName=Tunnel Floor\nFileName=tunnel\nTilesInSet=4\n\n\
     [TileSet0002]\nSetName=Waterfalls-C\nFileName=wfc\nTilesInSet=4\n\n\
     [Tunnel Floor]\n\
     Tile01Anim=TUNTOP01\nTile01XOffset=-48\nTile01YOffset=-37\n\
     Tile01AttachesTo=2\nTile01ZAdjust=-10\n\
     Tile02Anim=TUNTOP02\nTile02XOffset=48\nTile02YOffset=-37\n\
     Tile02AttachesTo=10\nTile02ZAdjust=-10\n\n\
     [Waterfalls-C]\n\
     Tile04Anim=WC04X\nTile04XOffset=-23\nTile04YOffset=-5\n\
     Tile04AttachesTo=1\nTile04ZAdjust=0\n"
        .to_string()
}

#[test]
fn gsi_13_04_tile_anims_come_from_the_setname_section() {
    let lookup = parse_tileset_ini(make_tile_anim_ini().as_bytes(), "tem").unwrap();

    // Tileset 1 starts at tile_id 1 (tileset 0 holds one tile).
    let first = lookup.tile_anim(1).expect("Tile01 block");
    assert_eq!(first.anim_name, "TUNTOP01");
    assert_eq!(first.x_offset, -48);
    assert_eq!(first.y_offset, -37);
    assert_eq!(first.attaches_to, 2);
    assert_eq!(first.z_adjust, -10);

    let second = lookup.tile_anim(2).expect("Tile02 block");
    assert_eq!(second.anim_name, "TUNTOP02");
    assert_eq!(second.attaches_to, 10);

    // Tiles 03/04 of that set declare nothing.
    assert_eq!(lookup.tile_anim(3), None);
    assert_eq!(lookup.tile_anim(4), None);

    // Tileset 2 starts at tile_id 5; only its 4th tile carries a block, and the
    // ordinal in the key is 1-based *within the tileset*.
    assert_eq!(lookup.tile_anim(5), None);
    let waterfall = lookup.tile_anim(8).expect("Tile04 block");
    assert_eq!(waterfall.anim_name, "WC04X");
    assert_eq!(waterfall.x_offset, -23);
    assert_eq!(waterfall.y_offset, -5);
    assert_eq!(waterfall.attaches_to, 1);

    // A tileset whose SetName names no section has no animations at all.
    assert_eq!(lookup.tile_anim(0), None);
}

#[test]
fn gsi_13_04_tile_anim_keys_are_ignored_without_a_name() {
    // The loader gates the whole block on `Tile%02dAnim` reading back a
    // non-empty string, so offsets alone never produce an attachment.
    let ini = "[TileSet0000]\nSetName=Ghost\nFileName=g\nTilesInSet=2\n\n\
               [Ghost]\n\
               Tile01XOffset=17\nTile01AttachesTo=0\n\
               Tile02Anim=\nTile02AttachesTo=0\n";
    let lookup = parse_tileset_ini(ini.as_bytes(), "tem").unwrap();
    assert_eq!(lookup.tile_anim(0), None);
    assert_eq!(lookup.tile_anim(1), None);
}

#[test]
fn gsi_13_04_tile_anim_numeric_keys_fall_back_to_constructor_defaults() {
    // Absent numeric keys keep the tile type's constructed values: offsets and
    // ZAdjust at 0, AttachesTo at the -1 sentinel that matches no sub-tile.
    let ini = "[TileSet0000]\nSetName=Bare\nFileName=b\nTilesInSet=1\n\n\
               [Bare]\nTile01Anim=WA01X\n";
    let lookup = parse_tileset_ini(ini.as_bytes(), "tem").unwrap();
    let anim = lookup.tile_anim(0).expect("anim block");
    assert_eq!(anim.anim_name, "WA01X");
    assert_eq!(anim.x_offset, 0);
    assert_eq!(anim.y_offset, 0);
    assert_eq!(anim.z_adjust, 0);
    assert_eq!(anim.attaches_to, TILE_ANIM_NO_SUBTILE);
}

#[test]
fn gsi_13_04_tile_anim_block_is_not_read_from_the_tileset_section() {
    let ini = "[TileSet0000]\nSetName=Grass\nFileName=clear\nTilesInSet=1\n\
               Tile01Anim=WA01X\nTile01AttachesTo=0\n";
    let lookup = parse_tileset_ini(ini.as_bytes(), "tem").unwrap();
    assert_eq!(lookup.tile_anim(0), None);
}
