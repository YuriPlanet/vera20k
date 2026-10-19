//! Full original LightConvert556090 RGB565 controls and live trigger consumers.
//! Steam gamemd.exe SHA256
//! 3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600.
//! Native controls use tools.palette_oracle's machine/call at556090 with
//! declared N27/N53 objects, RGB(i,i*73 mod256,255-i), MMX and liveCW0E7F.
//! Rows0/middle/last, indices64/128/200. No scene/GPU parity claim.
use super::*;
use crate::map::map_file::MapFile;
use crate::render::palette_light::PaletteLight;
use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
use crate::sim::scenario_session::ScenarioLightingProfile;
use crate::sim::world::FrameEffects;

fn global(palette: PaletteUpdate, state: ScenarioLightingState) -> LightingEvent {
    LightingEvent::Global {
        state,
        cell_profile: ScenarioLightingProfile::Normal,
        palette,
        constructor_rgb: None,
    }
}

#[test]
fn twenty_original_palette_histories_keep_mutable_normal_rgb_and_native_pixels() {
    // Literal native output. Stage names describe explicit RGB operations;
    // effect flags and the palette registries are outside these controls.
    let records: [(usize, &str, [i32; 3], [u16; 9]); 20] = [
        (
            0,
            "birth_a",
            [512, 640, 768],
            [0, 0, 0, 8530, 17036, 24613, 17055, 34072, 51274],
        ),
        (
            1,
            "birth_neutral",
            [1000, 1000, 1000],
            [0, 0, 0, 14839, 31727, 49190, 31743, 65535, 63597],
        ),
        (
            0,
            "normal_tint",
            [1230, 870, 650],
            [0, 0, 0, 18863, 39786, 61476, 39807, 65268, 63592],
        ),
        (
            1,
            "normal_tint",
            [1230, 870, 650],
            [0, 0, 0, 18863, 39786, 61476, 39807, 65268, 63592],
        ),
        (
            2,
            "birth_after_tint",
            [960, 640, 512],
            [0, 0, 0, 14668, 31368, 47139, 31384, 62736, 63559],
        ),
        (
            0,
            "ion",
            [300, 400, 750],
            [0, 0, 0, 4305, 8587, 14341, 8607, 19255, 28714],
        ),
        (
            1,
            "ion",
            [300, 400, 750],
            [0, 0, 0, 4305, 8587, 14341, 8607, 19255, 28714],
        ),
        (
            2,
            "ion",
            [300, 400, 750],
            [0, 0, 0, 4305, 8587, 14341, 8607, 19255, 28714],
        ),
        (
            0,
            "restore_normal",
            [1230, 870, 650],
            [0, 0, 0, 18863, 39786, 61476, 39807, 65268, 63592],
        ),
        (
            1,
            "restore_normal",
            [1230, 870, 650],
            [0, 0, 0, 18863, 39786, 61476, 39807, 65268, 63592],
        ),
        (
            2,
            "restore_normal",
            [960, 640, 512],
            [0, 0, 0, 14668, 31368, 47139, 31384, 62736, 63559],
        ),
        (
            0,
            "nuke",
            [1750, 1500, 1250],
            [0, 0, 0, 27389, 56819, 63560, 56831, 65535, 63665],
        ),
        (
            1,
            "nuke",
            [1750, 1500, 1250],
            [0, 0, 0, 27389, 56819, 63560, 56831, 65535, 63665],
        ),
        (
            2,
            "nuke",
            [1750, 1500, 1250],
            [0, 0, 0, 27389, 56819, 63560, 56831, 65535, 63665],
        ),
        (
            0,
            "normal_during_alternate",
            [0, 2000, 800],
            [0, 0, 0, 1011, 2028, 101, 2047, 2041, 234],
        ),
        (
            1,
            "normal_during_alternate",
            [0, 2000, 800],
            [0, 0, 0, 1011, 2028, 101, 2047, 2041, 234],
        ),
        (
            2,
            "normal_during_alternate",
            [0, 2000, 800],
            [0, 0, 0, 1011, 2028, 101, 2047, 2041, 234],
        ),
        (
            0,
            "restore_normal_again",
            [0, 2000, 800],
            [0, 0, 0, 1011, 2028, 101, 2047, 2041, 234],
        ),
        (
            1,
            "restore_normal_again",
            [0, 2000, 800],
            [0, 0, 0, 1011, 2028, 101, 2047, 2041, 234],
        ),
        (
            2,
            "restore_normal_again",
            [0, 2000, 800],
            [0, 0, 0, 1011, 2028, 101, 2047, 2041, 234],
        ),
    ];
    let terrain =
        crate::map::resolved_terrain::test_grid(3, 1, crate::map::resolved_terrain::test_flat_cell);
    let mut lights = MatchLighting::default();
    let mut previous_stage = "";
    let keys: [[i32; 3]; 3] = [[512, 640, 768], [1000; 3], [960, 640, 512]];
    for (object, stage, expected_rgb, expected_pixels) in records {
        if stage.starts_with("birth") {
            lights.grid.insert_profiled_light(
                (object as u16, 0),
                keys[object].map(|v| v as f32 / 1000.0),
                1.0,
            );
        } else if stage != previous_stage {
            let palette = match stage {
                "normal_tint" => PaletteUpdate::Normal([1230, 870, 650]),
                "ion" => PaletteUpdate::Alternate([300, 400, 750]),
                "nuke" => PaletteUpdate::Alternate([1750, 1500, 1250]),
                "normal_during_alternate" => PaletteUpdate::Normal([-10, 3000, 800]),
                "restore_normal" | "restore_normal_again" => PaletteUpdate::RestoreNormal,
                _ => panic!("unexpected native stage"),
            };
            lights.apply_events(
                &terrain,
                &[global(palette, ScenarioLightingState::default())],
            );
        }
        previous_stage = stage;
        let cell = (object as u16, 0);
        let retained = lights.grid.cell_light_at(cell).unwrap();
        assert_eq!(retained.rgb_key, keys[object], "{stage}/{object}");
        assert_eq!(
            lights.grid.palette_rgb(retained.profile_id),
            expected_rgb,
            "{stage}/{object}"
        );
        let rows = if object == 0 { 27 } else { 53 };
        let packet = PaletteLight::cell(&lights.grid, cell, false);
        assert_eq!(packet.rows(), rows);
        let mut pixels = Vec::new();
        for brightness in [0, 1000, 2000] {
            let packet = packet.with_brightness(brightness);
            for index in [64u8, 128, 200] {
                pixels.push(packet.rgb565(
                    [index, index.wrapping_mul(73), 255 - index],
                    index,
                    127,
                ));
            }
        }
        assert_eq!(pixels, expected_pixels, "{stage}/{object}");
    }
}

#[test]
fn mutable_lookup_keeps_old_cell_pointers_and_row_counts_while_new_samples_get_new_converts() {
    //544E94 returns neutral index0;544EF8 compares normal cache from
    // index1, returning the first matching Convert with its existing N.
    let mut grid = CellLightGrid::new();
    grid.insert_profiled_light((0, 0), [0.512, 0.640, 0.768], 1.0);
    let old = grid.cell_light_at((0, 0)).unwrap().profile_id;
    grid.retint_normal_rgb([960, 640, 512]);
    grid.insert_profiled_light((1, 0), [0.960, 0.640, 0.512], 1.0);
    assert_eq!(grid.cell_light_at((1, 0)).unwrap().profile_id, old);
    assert_eq!(PaletteLight::cell(&grid, (1, 0), false).rows(), 27);
    grid.insert_profiled_light((2, 0), [0.512, 0.640, 0.768], 1.0);
    let new = grid.cell_light_at((2, 0)).unwrap().profile_id;
    assert_ne!(new, old);
    assert_eq!(grid.palette_rgb(old), [960, 640, 512]);
    assert_eq!(grid.palette_rgb(new), [512, 640, 768]);
    grid.insert_profiled_light((3, 0), [1.0; 3], 1.0);
    assert_eq!(
        grid.cell_light_at((3, 0)).unwrap().profile_id,
        grid.profiles().default_profile_id()
    );
    assert_eq!(PaletteLight::cell(&grid, (3, 0), false).rows(), 53);
}

#[test]
fn live_tag_rgb_overrides_reach_packets_and_survive_ambient_writes_during_storm() {
    let map = MapFile::from_bytes(b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Basic]\nNewINIFormat=4\n[Triggers]\nRGB=Neutral,<none>,RGB,0,1,1,1,0\n[Events]\nRGB=1,13,0,0\n[Actions]\nRGB=4,142,0,120,0,0,0,0,A,143,0,80,0,0,0,0,A,144,0,60,0,0,0,0,A,73,0,80,0,0,0,0,A\n[Tags]\nRGBTAG=0,RGB,RGB\n").unwrap();
    let mut sim = Simulation::with_seed(0);
    sim.initialize_trigger_definitions(
        &map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
    );
    sim.classify_map_triggers(None);
    sim.session.set_started(true);
    // Declared raging-storm fixture; its lifecycle has separate tests.
    let owner = sim.interner.intern("Americans");
    sim.lightning_storm =
        crate::sim::superweapon::lightning_storm::LightningStorm::raging_for_test(owner, (0, 0));
    sim.session.lighting.select(ScenarioLightingProfile::Ion);
    let terrain =
        crate::map::resolved_terrain::test_grid(3, 1, crate::map::resolved_terrain::test_flat_cell);
    let mut lights = MatchLighting::default();
    lights
        .grid
        .insert_profiled_light((0, 0), [0.512, 0.640, 0.768], 1.0);
    sim.publish_global_lighting(PaletteUpdate::from_scenario(&sim.session.lighting));
    lights.apply_events(&terrain, &sim.lighting_sources.pending);
    sim.discard_lighting_events();
    sim.advance_triggers(None, None, FrameEffects::default());
    assert_eq!(sim.lighting_sources.pending.len(), 3);
    lights.apply_events(&terrain, &sim.lighting_sources.pending);
    sim.discard_lighting_events();
    //73 changes normal Ambient after the final RGB publication. Reconciling
    // that state must not infer a new Ion palette operation.
    lights.refresh(&terrain, &sim, 2);
    let before = lights.grid.cell_light_at((0, 0)).unwrap().profile_id;
    assert_eq!(lights.grid.palette_rgb(before), [1200, 800, 600]);
    let brightness = lights.grid.cell_light_at((0, 0)).unwrap().top_scalar;
    let expected = PaletteLight::color_scheme([1200, 800, 600], brightness);
    assert_eq!(body_palette_light(&lights.grid, (0, 0), 0), expected);
    assert_eq!(
        building_palette_light(&lights.grid, (0, 0), None, false),
        expected
    );
    let art = ArtRegistry::from_ini(&IniFile::from_str("[ALT]\nAltPalette=yes\n"));
    // Default N1 schemes are skipped by53AD4D, independently of N53 and
    // cell caches (68C710 RGB1000;4232E2 selects B054D4[0]+30C).
    assert_eq!(
        anim_palette_light(&lights.grid, (0, 0), art.anim_runtime_config("ALT"), false),
        PaletteLight::new([1000; 3], 1, brightness, true)
    );
    assert_eq!(
        anim_palette_light(&lights.grid, (0, 0), None, false),
        PaletteLight::plain(53, brightness)
    );
    //555E50 constructs a new Convert with Storm RGB even though the
    // preceding normal operation switched existing Converts out of Ion.
    lights
        .grid
        .insert_profiled_light((1, 0), [0.960, 0.640, 0.512], 1.0);
    let after = lights.grid.cell_light_at((1, 0)).unwrap().profile_id;
    assert_eq!(lights.grid.palette_rgb(after), [300, 400, 750]);
    assert_eq!(lights.grid.palette_rgb(before), [1200, 800, 600]);
    lights.apply_events(
        &terrain,
        &[global(
            PaletteUpdate::RestoreNormal,
            ScenarioLightingState::default(),
        )],
    );
    assert_eq!(lights.grid.palette_rgb(before), [1200, 800, 600]);
    assert_eq!(lights.grid.palette_rgb(after), [960, 640, 512]);
}
