//! Focused tests for sidebar chrome asset authority and retail provenance.

use std::path::PathBuf;

use super::{
    ALLIED_SIDE_ARCHIVE_ORDER, SIDE_TWO_ARCHIVE_ORDER, SidebarSideRoute, build_gclock_cpu_atlas,
    is_generic_sidebar_shp_name, resolve_sidebar_asset_in_order,
};
use crate::app::presentation::sidebar_build::build_gclock_instance;
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::assets::mix_archive::MixArchive;
use crate::assets::mix_hash::mix_hash;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::render::sidebar_chrome::SidebarTheme;
use crate::sidebar::Rect;
use crate::util::config::GameConfig;

fn retail_ra2_dir() -> PathBuf {
    std::env::var_os("RA2_DIR")
        .map(PathBuf::from)
        .or_else(|| GameConfig::load().ok().map(|config| config.paths.ra2_dir))
        .expect("set RA2_DIR or provide config.toml for the ignored retail test")
}

fn test_mix(name: &str, body: &[u8]) -> MixArchive {
    let mut data = Vec::new();
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&(body.len() as u32).to_le_bytes());
    data.extend_from_slice(&mix_hash(name).to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&(body.len() as u32).to_le_bytes());
    data.extend_from_slice(body);
    MixArchive::from_bytes(data).expect("synthetic MIX")
}

#[test]
fn side_route_prefers_md_then_base_then_neutral() {
    let md = test_mix("piece.shp", b"md");
    let base = test_mix("piece.shp", b"base");
    let neutral = test_mix("piece.shp", b"neutral");
    let resolved = resolve_sidebar_asset_in_order(
        [
            ("sidec02md.mix", &md),
            ("sidec02.mix", &base),
            ("sidenc02.mix", &neutral),
        ],
        "piece.shp",
    )
    .expect("MD winner");
    assert_eq!(resolved.archive_name, "sidec02md.mix");
    assert_eq!(resolved.bytes, b"md");

    let unrelated_md = test_mix("other.shp", b"other");
    let resolved = resolve_sidebar_asset_in_order(
        [
            ("sidec02md.mix", &unrelated_md),
            ("sidec02.mix", &base),
            ("sidenc02.mix", &neutral),
        ],
        "piece.shp",
    )
    .expect("base fallback");
    assert_eq!(resolved.archive_name, "sidec02.mix");
    assert_eq!(resolved.bytes, b"base");

    let unrelated_base = test_mix("other.shp", b"other");
    let resolved = resolve_sidebar_asset_in_order(
        [
            ("sidec02md.mix", &unrelated_md),
            ("sidec02.mix", &unrelated_base),
            ("sidenc02.mix", &neutral),
        ],
        "piece.shp",
    )
    .expect("neutral fallback");
    assert_eq!(resolved.archive_name, "sidenc02.mix");
    assert_eq!(resolved.bytes, b"neutral");
}

#[test]
fn generic_route_is_bounded_away_from_yuri_theme_art() {
    for name in [
        "SIDE1.SHP",
        "SIDE2.SHP",
        "SIDE2B.SHP",
        "SIDEBTTN.SHP",
        "SIDE3.SHP",
        "TAB00.SHP",
        "TAB01.SHP",
        "TAB02.SHP",
        "TAB03.SHP",
        "REPAIR.SHP",
        "SELL.SHP",
        "R-UP.SHP",
        "R-DN.SHP",
        "POWERP.SHP",
        "GCLOCK2.SHP",
        "TOP.SHP",
        "CREDITS.SHP",
        "ADDON.SHP",
        "DIPLOBTN.SHP",
        "OPTBTN.SHP",
    ] {
        assert!(is_generic_sidebar_shp_name(name), "{name}");
    }

    for name in [
        "RADAR.SHP",
        "RADARY.SHP",
        "BKGDLGY.SHP",
        "BKGDMDY.SHP",
        "BKGDSMY.SHP",
        "TABS.SHP",
        "POWER.SHP",
        "UNKNOWN.SHP",
    ] {
        assert!(!is_generic_sidebar_shp_name(name), "{name}");
    }
}

#[test]
fn theme_routes_match_active_side_mapping() {
    assert_eq!(
        ALLIED_SIDE_ARCHIVE_ORDER,
        &["sidec01md.mix", "sidec01.mix", "sidenc01.mix"]
    );
    assert_eq!(
        SIDE_TWO_ARCHIVE_ORDER,
        &["sidec02md.mix", "sidec02.mix", "sidenc02.mix"]
    );
}

fn resolved_shp(route: SidebarSideRoute<'_>, name: &str) -> (&'static str, ShpFile) {
    let resolved = route.resolve_generic_shp(name).expect(name);
    let shp = ShpFile::from_bytes(resolved.bytes).expect(name);
    (resolved.archive_name, shp)
}

fn atlas_cell_has_alpha(
    atlas: &super::CpuGclockAtlas,
    frame_index: usize,
    cell_width: u32,
    cell_height: u32,
) -> bool {
    let cell_x = (frame_index as u32 % 8) * cell_width;
    let cell_y = (frame_index as u32 / 8) * cell_height;
    (cell_y..cell_y + cell_height).any(|y| {
        (cell_x..cell_x + cell_width).any(|x| {
            let alpha = ((y * atlas.width + x) * 4 + 3) as usize;
            atlas.rgba.get(alpha).copied().unwrap_or_default() != 0
        })
    })
}

#[test]
#[ignore = "requires the configured stock retail RA2/YR install"]
fn retail_yuri_generic_route_uses_side_two_and_builds_production_clock() {
    let assets = AssetManager::new(&retail_ra2_dir(), MediaArchiveMode::STOCK_DIGITAL)
        .expect("load retail asset stack");
    let allied = SidebarSideRoute::for_theme(&assets, SidebarTheme::Allied);
    let soviet = SidebarSideRoute::for_theme(&assets, SidebarTheme::Soviet);
    let yuri = SidebarSideRoute::for_theme(&assets, SidebarTheme::Yuri);

    let (allied_tab_source, allied_tab) = resolved_shp(allied, "TAB00.SHP");
    let (soviet_tab_source, soviet_tab) = resolved_shp(soviet, "TAB00.SHP");
    let (yuri_tab_source, yuri_tab) = resolved_shp(yuri, "TAB00.SHP");
    assert_eq!(allied_tab_source, "sidec01.mix");
    assert_eq!(soviet_tab_source, "sidec02.mix");
    assert_eq!(yuri_tab_source, "sidec02.mix");
    assert_eq!((allied_tab.width, allied_tab.height), (28, 27));
    assert_eq!((soviet_tab.width, soviet_tab.height), (32, 28));
    assert_eq!((yuri_tab.width, yuri_tab.height), (32, 28));

    for (name, dimensions) in [
        ("REPAIR.SHP", (52, 32)),
        ("SELL.SHP", (52, 32)),
        ("R-UP.SHP", (46, 27)),
        ("R-DN.SHP", (46, 27)),
        ("POWERP.SHP", (16, 2)),
    ] {
        let (source, shp) = resolved_shp(yuri, name);
        assert_eq!(source, "sidec02.mix", "{name}");
        assert_eq!((shp.width, shp.height), dimensions, "{name}");
    }

    let generic_palette_source = yuri.resolve("SIDEBAR.PAL").expect("generic palette");
    assert_eq!(generic_palette_source.archive_name, "sidec02.mix");
    let generic_palette = Palette::from_bytes(generic_palette_source.bytes).expect("SIDEBAR.PAL");

    let yuri_theme = assets.archive("sidec02md.mix").expect("Yuri theme archive");
    assert!(yuri_theme.get_by_name("RADARYURI.PAL").is_some());
    assert!(yuri_theme.get_by_name("RADARY.SHP").is_some());
    assert!(yuri_theme.get_by_name("BKGDLGY.SHP").is_some());
    assert!(!is_generic_sidebar_shp_name("RADARY.SHP"));
    assert!(!is_generic_sidebar_shp_name("BKGDLGY.SHP"));

    let atlas = build_gclock_cpu_atlas(yuri, &generic_palette).expect("Yuri GCLOCK2 atlas");
    assert_eq!(atlas.source_archive, "sidec02.mix");
    assert_eq!((atlas.width, atlas.height), (480, 336));
    assert_eq!(atlas.frames.len(), 55);
    assert!(!atlas_cell_has_alpha(&atlas, 0, 60, 48));
    assert!(atlas_cell_has_alpha(&atlas, 1, 60, 48));
    assert!(atlas_cell_has_alpha(&atlas, 54, 60, 48));

    let instance = build_gclock_instance(
        &atlas.frames,
        0.5,
        Rect {
            x: 10.0,
            y: 20.0,
            w: 60.0,
            h: 48.0,
        },
        [0.0, 0.0],
    )
    .expect("production GCLOCK instance");
    assert_eq!(instance.position, [10.0, 20.0]);
    assert_eq!(instance.size, [60.0, 48.0]);
    assert_eq!(instance.uv_origin, atlas.frames[28].uv_origin);
    assert_eq!(instance.uv_size, atlas.frames[28].uv_size);
}

#[test]
#[ignore = "requires the configured stock retail RA2/YR install"]
fn retail_in_game_shell_art_preserves_side_route_and_frame_transparency() {
    let assets = AssetManager::new(&retail_ra2_dir(), MediaArchiveMode::STOCK_DIGITAL)
        .expect("load retail asset stack");
    // Independent stock MIX/SHP reads: both SIDE2B canvases are 168x50;
    // SIDEBTTN has three 125x25 frames, with 28 skipped pixels per Allied
    // frame and none per Soviet frame. Yuri shares the Soviet generic art
    // and SIDEBAR.PAL, not the RADARYURI.PAL used by its radar.
    for (theme, archive, skipped_pixels) in [
        (SidebarTheme::Allied, "sidec01.mix", 28),
        (SidebarTheme::Soviet, "sidec02.mix", 0),
        (SidebarTheme::Yuri, "sidec02.mix", 0),
    ] {
        let route = SidebarSideRoute::for_theme(&assets, theme);
        for name in ["SIDE2B.SHP", "SIDEBTTN.SHP", "SIDEBAR.PAL"] {
            assert_eq!(route.resolve(name).expect(name).archive_name, archive);
        }
        let palette = super::decode_sidebar_palette(route.resolve("SIDEBAR.PAL").unwrap().bytes)
            .expect("native sidebar palette");
        let art = super::in_game_shell::load(route, &palette);
        let tile = art.panel_tile.as_ref().expect("SIDE2B frame zero");
        assert_eq!((tile.width, tile.height), (168, 50));
        assert!(tile.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
        for button in &art.buttons {
            let button = button.as_ref().expect("SIDEBTTN frame");
            assert_eq!((button.width, button.height), (125, 25));
            assert_eq!(
                button
                    .rgba
                    .chunks_exact(4)
                    .filter(|pixel| pixel[3] == 0)
                    .count(),
                skipped_pixels,
            );
        }
        for states in art.buttons.windows(2) {
            assert_ne!(
                states[0].as_ref().unwrap().rgba,
                states[1].as_ref().unwrap().rgba
            );
        }
    }
}

#[test]
fn original_n1_tables_drive_sidebar_palette_and_skip_alpha() {
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Packet {
        cases: Vec<Case>,
    }
    #[derive(Deserialize)]
    struct Case {
        raw: Vec<u8>,
        rgb: Vec<u8>,
        words: Vec<u16>,
    }
    let packet: Packet = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/palette.json",
    ))
    .unwrap();
    for c in packet.cases {
        let p = super::decode_sidebar_palette(&c.raw).unwrap();
        assert_eq!(p.colors.len(), c.words.len());
        for (i, color) in p.colors.iter().enumerate() {
            let native = c.words[i];
            let actual = ((u16::from(color.r) >> 3) << 11)
                | ((u16::from(color.g) >> 2) << 5)
                | (u16::from(color.b) >> 3);
            assert_eq!(actual, native, "palette index{i}");
            assert_eq!(color.a, 255, "index-zero suppression belongs to SHP");
            assert_eq!(
                crate::render::palette_light::PaletteLight::plain(1, 1000).rgb565(
                    c.rgb[i * 3..i * 3 + 3].try_into().unwrap(),
                    i as u8,
                    127
                ),
                native
            );
        }
    }
}

#[test]
#[ignore = "requires the configured stock retail RA2/YR install"]
fn retail_modal_backgrounds_use_ui_palette_for_every_theme_and_size() {
    let assets = AssetManager::new(&retail_ra2_dir(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    for (theme, archive_name, names, palette_name, old_palette_name) in [
        (
            SidebarTheme::Allied,
            "sidec01.mix",
            ("BKGDLG.SHP", "BKGDMD.SHP", "BKGDSM.SHP"),
            "UIBKGD.PAL",
            "SIDEBAR.PAL",
        ),
        (
            SidebarTheme::Soviet,
            "sidec02.mix",
            ("BKGDLG.SHP", "BKGDMD.SHP", "BKGDSM.SHP"),
            "UIBKGD.PAL",
            "SIDEBAR.PAL",
        ),
        (
            SidebarTheme::Yuri,
            "sidec02md.mix",
            ("BKGDLGY.SHP", "BKGDMDY.SHP", "BKGDSMY.SHP"),
            "UIBKGDY.PAL",
            "RADARYURI.PAL",
        ),
    ] {
        let archive = assets.archive(archive_name).unwrap();
        let (entries, identity) =
            super::in_game_shell::load_backgrounds(&assets, &archive, archive_name, theme, names)
                .unwrap();
        assert_eq!(identity.logical_name, palette_name);
        assert_eq!(identity.source_archive.as_deref(), Some(archive_name));
        let raw_palette = archive.get_by_name(palette_name).unwrap();
        let old_palette =
            super::decode_sidebar_palette(archive.get_by_name(old_palette_name).unwrap()).unwrap();
        for (name, entry) in [names.0, names.1, names.2].into_iter().zip(entries) {
            let entry = entry.unwrap();
            let shp = ShpFile::from_bytes(archive.get_by_name(name).unwrap()).unwrap();
            let frame = &shp.frames[0];
            assert_eq!((frame.frame_x, frame.frame_y), (0, 0));
            assert_eq!(
                (frame.frame_width, frame.frame_height),
                (shp.width, shp.height)
            );
            assert_eq!(entry.rgba.len(), frame.pixels.len() * 4);
            // Compare every production pixel to the retail palette triplet's
            // original72ADE0 six-bit expansion and N=1 RGB565 store. The shared
            // converter arithmetic also has native palette.json coverage above.
            for (&index, rgba) in frame.pixels.iter().zip(entry.rgba.chunks_exact(4)) {
                assert_eq!(rgba[3], if index == 0 { 0 } else { 255 });
                if index == 0 {
                    continue;
                }
                let p = &raw_palette[index as usize * 3..][..3];
                let expected = ((u16::from(p[0]) >> 1) << 11)
                    | (u16::from(p[1]) << 5)
                    | (u16::from(p[2]) >> 1);
                let actual = ((u16::from(rgba[0]) >> 3) << 11)
                    | ((u16::from(rgba[1]) >> 2) << 5)
                    | (u16::from(rgba[2]) >> 3);
                assert_eq!(actual, expected, "{archive_name}/{name}, index{index}");
            }
            let old = super::render_shp(&shp, &old_palette, 0).unwrap();
            assert_ne!(
                entry.rgba, old.rgba,
                "{archive_name}/{name}: old radar palette must differ"
            );
        }
    }
}

#[test]
#[ignore = "requires the configured original stock retail archives"]
fn retail_radar_histories_match_original_4912b0_stores() {
    use crate::util::sha256::sha256_hex;
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Packet {
        cases: Vec<Case>,
    }
    #[derive(Deserialize)]
    struct Case {
        archive: String,
        name: String,
        palette: String,
        source_sha256: String,
        decoded_frame_sha256: Vec<String>,
        zero_counts: Vec<usize>,
        sequences: Vec<Sequence>,
    }
    #[derive(Deserialize)]
    struct Sequence {
        name: String,
        frames: Vec<usize>,
        output_rgb565_sha256: Vec<String>,
    }
    let packet: Packet = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/radar_surface.json",
    ))
    .unwrap();
    let assets = AssetManager::new(&retail_ra2_dir(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    for c in packet.cases {
        let archive = assets.archive(&c.archive).unwrap();
        let raw = archive.get_by_name(&c.name).unwrap();
        assert_eq!(sha256_hex(raw), c.source_sha256);
        let shp = ShpFile::from_bytes(raw).unwrap();
        let palette =
            super::decode_sidebar_palette(archive.get_by_name(&c.palette).unwrap()).unwrap();
        let frames: Vec<_> = (0..shp.frames.len())
            .map(|i| super::render_shp(&shp, &palette, i).unwrap().rgba)
            .collect();
        for (i, f) in shp.frames.iter().enumerate() {
            assert_eq!(sha256_hex(&f.pixels), c.decoded_frame_sha256[i]);
            assert_eq!(
                f.pixels.iter().filter(|&&p| p == 0).count(),
                c.zero_counts[i]
            );
            assert_eq!(
                frames[i].chunks_exact(4).filter(|p| p[3] == 0).count(),
                c.zero_counts[i]
            );
        }
        assert!(
            c.zero_counts[12..].iter().all(|&n| n == 0),
            "closing overwrites minimap before first zero frame"
        );
        for seq in c.sequences {
            let mut radar =
                crate::render::radar_anim::RadarPresentation::new(frames.clone(), 168, 110)
                    .unwrap();
            for (step, (&frame, expected)) in
                seq.frames.iter().zip(&seq.output_rgb565_sha256).enumerate()
            {
                if step == 0 {
                    assert_eq!(frame, 0);
                } else {
                    let previous = seq.frames[step - 1];
                    assert_eq!(
                        frame.abs_diff(previous),
                        1,
                        "one native draw per history step"
                    );
                    radar.set_has_radar(frame > previous);
                    assert!(radar.advance_draw((step as u64 - 1) * 64));
                }
                let bytes: Vec<u8> = radar
                    .surface
                    .rgba
                    .chunks_exact(4)
                    .flat_map(|p| {
                        (((u16::from(p[0]) >> 3) << 11)
                            | ((u16::from(p[1]) >> 2) << 5)
                            | (u16::from(p[2]) >> 3))
                            .to_le_bytes()
                    })
                    .collect();
                assert_eq!(
                    sha256_hex(&bytes),
                    *expected,
                    "{} {} frame{}",
                    c.archive,
                    seq.name,
                    frame
                );
            }
        }
    }
}
