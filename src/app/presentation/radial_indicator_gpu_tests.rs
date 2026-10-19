//! Offscreen production consumer for the armed native superweapon range rings.

use super::{SuperweaponRangePulse, build};
use crate::map::houses::HouseColorMap;
use crate::render::batch::BatchRenderer;
use crate::render::radial_indicator::radius_pixels;
use crate::render::surface_line::SurfaceLineViewport;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, encoded, seed_depth_grid};
use crate::sim::world::Simulation;
use serde_json::Value;

fn native_controls() -> Value {
    serde_json::from_str::<Value>(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/radial_indicator.json",
    ))
    .unwrap()["superweapon_range"]
        .clone()
}

fn native_radii(control: &Value) -> Vec<u32> {
    control["radii_f32_bits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|bits| u32::from_str_radix(bits.as_str().unwrap(), 16).unwrap())
        .collect()
}

fn sim(rules: &crate::rules::ruleset::RuleSet) -> Simulation {
    let mut sim = Simulation::with_seed(1);
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    for name in ["Americans", "British"] {
        let owner = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 0, 10,
            ));
    }
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim
}

fn build_nuke(
    sim: &Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    viewport: SurfaceLineViewport,
    pulse: &mut SuperweaponRangePulse,
) -> Vec<crate::render::radial_indicator::RadialIndicatorDraw> {
    let colors = HouseColorMap::from([(
        "Americans".to_owned(),
        crate::rules::house_colors::HouseColorIndex(3),
    )]);
    build(
        Some(sim),
        Some(rules),
        &[],
        Some("NukeSpecial"),
        Some((16, 16)),
        pulse,
        &colors,
        viewport,
        0,
    )
}

#[test]
#[ignore = "requires GPU and retail RA2/YR INI; windowless native superweapon rings through production surface lines"]
fn armed_nuke_rings_reach_offscreen_surface_line_gpu() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let nuke = rules.super_weapon("NukeSpecial").unwrap();
    assert_eq!(nuke.range, 7.0);
    assert_eq!(nuke.line_multiplier, 2);
    let sim = sim(rules);
    let control = native_controls()["loop_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "nuke_special_retail_7_2")
        .unwrap()
        .clone();

    let size = [768, 768];
    let clip = [0, 0, size[0] as i32, size[1] as i32];
    let viewport_at_origin = SurfaceLineViewport {
        camera: [0; 2],
        clip,
        z_origin_y: 0,
        zoom: 1.0,
    };
    // Use the production cell/center path to place the target well inside this
    // offscreen surface, then rebuild with a fresh pulse for the actual draw.
    let mut probe_pulse = SuperweaponRangePulse::default();
    let probe = build_nuke(&sim, rules, viewport_at_origin, &mut probe_pulse);
    assert!(!probe.is_empty());
    let center = [size[0] as i32 / 2, size[1] as i32 / 2];
    let viewport = SurfaceLineViewport {
        camera: [
            probe[0].center[0] - center[0],
            probe[0].center[1] - center[1],
        ],
        ..viewport_at_origin
    };
    let mut pulse = SuperweaponRangePulse::default();
    let radials = build_nuke(&sim, rules, viewport, &mut pulse);
    assert_eq!(
        radials.len(),
        control["radii_f32_bits"].as_array().unwrap().len()
    );
    assert_eq!(
        radials[0].radius.to_bits(),
        0,
        "the first native ring has radius zero"
    );
    assert!(
        radials
            .iter()
            .all(|draw| !draw.pixel_radius && !draw.truncate_radius)
    );
    assert_eq!(
        radials
            .iter()
            .map(|draw| draw.radius.to_bits())
            .collect::<Vec<_>>(),
        native_radii(&control),
        "production builder must retain every native Nuke radius"
    );
    assert_eq!(radials[0].center, center);
    assert_ne!(
        radials[0].rgb, [0; 3],
        "the current House palette supplies visible color"
    );

    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(size));
    let color = gpu.target(size, format);
    let color_view = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let depth_view = depth.create_view(&Default::default());
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
    terrain.prepare(&gpu.device, &color, &depth_view, batch.camera_uniform());

    let background = encoded(0, format);
    let initial = background.repeat((size[0] * size[1]) as usize);
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &color,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &initial,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: Some(size[1]),
        },
        color.size(),
    );
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let depths = vec![0u16; (size[0] * size[1]) as usize];
    seed_depth_grid(&gpu, &mut encoder, &depth_view, &depths, size[0]);
    terrain.begin_surface_lines();
    let prepared = terrain.prepare_surface_lines(
        &gpu.device,
        &gpu.queue,
        std::iter::empty(),
        std::iter::empty(),
        &[],
        &[],
        &radials,
        viewport,
        |_| true,
        |_| u16::MAX,
    );
    terrain.draw_surface_lines(&mut encoder, &color_view, prepared);
    let reads = [
        gpu.read(&mut encoder, &color),
        gpu.read(&mut encoder, &depth),
    ];
    let output = gpu.finish(encoder, &reads, size);

    for draw in &radials {
        let radius =
            radius_pixels(draw.radius, draw.truncate_radius).expect("bounded retail radius");
        let x = (center[0] + radius) as usize;
        let y = center[1] as usize;
        let offset = (y * size[0] as usize + x) * 4;
        let pixel = &output[0][offset..offset + 4];
        assert_ne!(pixel, background, "ring cardinal pixel at radius {radius}");
    }

    let expected_depth: Vec<u8> = depths
        .iter()
        .flat_map(|&word| crate::render::native_z::stored_depth(word).to_le_bytes())
        .collect();
    assert_eq!(
        output[1], expected_depth,
        "radial rings leave destination Z unchanged"
    );
}
