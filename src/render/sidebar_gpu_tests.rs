//! Actual batch-shader readback for retained sidebar pixels. This harness uses
//! the production shader and SpriteInstance; native frame goldens are checked
//! separately through the production SHP/palette preparation.
use super::{
    batch::{CameraUniform, SpriteInstance},
    frame_readback::PendingBgra8Readback,
    radar_surface::RadarSurface,
};
use bytemuck::Zeroable;
use std::{
    mem::{offset_of, size_of},
    time::Duration,
};
use wgpu::util::DeviceExt;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
struct Layer {
    quad: SpriteInstance,
    rgba: Vec<u8>,
    size: [u32; 2],
}
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}
fn extent(s: [u32; 2]) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: s[0],
        height: s[1],
        depth_or_array_layers: 1,
    }
}
impl Gpu {
    fn new() -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("explicit sidebar GPU test requires adapter");
        eprintln!("Sidebar GPU adapter: {:?}", adapter.get_info());
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        Self { device, queue }
    }
    fn texture(
        &self,
        bytes: &[u8],
        size: [u32; 2],
        format: wgpu::TextureFormat,
    ) -> wgpu::TextureView {
        self.device
            .create_texture_with_data(
                &self.queue,
                &wgpu::TextureDescriptor {
                    label: None,
                    size: extent(size),
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                bytes,
            )
            .create_view(&Default::default())
    }
    fn render(&self, layers: &[Layer], size: [u32; 2], origin: [f32; 2]) -> Vec<u8> {
        let mut camera = CameraUniform::zeroed();
        camera.screen_size = size.map(|s| s as f32);
        camera.camera_pos = origin;
        camera.zoom = 1.;
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::bytes_of(&camera),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(
                    super::tactical_shader::world_source(include_str!("batch_shader.wgsl")).into(),
                ),
            });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: size_of::<SpriteInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &INSTANCE_ATTRIBUTES,
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            });
        let (neutral_a, neutral_a_uniform) =
            super::shroud_buffer::neutral_gpu_source(&self.device, &self.queue);
        let camera_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&neutral_a),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: neutral_a_uniform.as_entire_binding(),
                },
            ],
        });
        let output = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = output.create_view(&Default::default());
        let sampler = self
            .device
            .create_sampler(&wgpu::SamplerDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for (i, layer) in layers.iter().enumerate() {
            let texture =
                self.texture(&layer.rgba, layer.size, wgpu::TextureFormat::Rgba8UnormSrgb);
            let indices = self.texture(&[1], [1, 1], wgpu::TextureFormat::R8Uint);
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(1),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&indices),
                    },
                ],
            });
            let instances = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::bytes_of(&layer.quad),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: if i == 0 {
                            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &camera_group, &[]);
            pass.set_bind_group(1, &group, &[]);
            pass.set_vertex_buffer(0, instances.slice(..));
            pass.draw(0..6, 0..1);
        }
        let readback = PendingBgra8Readback::encode(
            &self.device,
            &mut encoder,
            &output,
            FORMAT,
            size[0],
            size[1],
        )
        .unwrap();
        let submission = self.queue.submit([encoder.finish()]);
        readback
            .finish(&self.device, submission, Duration::from_secs(20))
            .unwrap()
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect()
    }
}
fn quad(position: [f32; 2], size: [f32; 2]) -> SpriteInstance {
    SpriteInstance {
        position,
        size,
        uv_size: [1.; 2],
        tint: [1.; 3],
        alpha: 1.,
        depth: 0.00048,
        ..Default::default()
    }
}
#[test]
#[ignore = "requires a wgpu adapter; executes the production batch shader"]
fn retained_radar_snapshot_matches_actual_gpu_crops_and_overlaps() {
    let gpu = Gpu::new();
    let source_size = [140, 108];
    let source: Vec<u8> = (0..source_size[0] * source_size[1])
        .flat_map(|i| {
            [
                (i % 256) as u8,
                ((i * 73) % 256) as u8,
                ((i * 191) % 256) as u8,
                if i % 37 == 0 { 0 } else { 255 },
            ]
        })
        .collect();
    for origin in [
        [0., 0.],
        [632.25, -413.75],
        [13852.375, 8181.625],
        [631.4999, -412.5001],
        [631.5001, -412.4999],
    ] {
        let mut cases = vec![
            ([16., 1.], [0., 0.], [140., 108.]),
            ([26., 11.], [10., 10.], [120., 88.]),
            ([-3., -4.], [0., 0.], [140., 108.]),
            ([137., 95.], [125., 98.], [15., 10.]),
        ]
        .into_iter()
        .map(|(offset, crop, area)| {
            (
                offset,
                [crop[0] / 140., crop[1] / 108.],
                [area[0] / 140., area[1] / 108.],
                area,
            )
        })
        .collect::<Vec<_>>();
        for (raw_w, raw_h) in [(300, 180), (180, 300), (173, 113), (140, 108)] {
            let surface = super::native_radar_surface::NativeRadarSurfaceGeometry::from_raw_rect(
                30, 20, raw_w, raw_h,
            )
            .unwrap();
            let copy = super::minimap_projection::generated_primary_copy_frame(surface, 140., 108.);
            cases.push((
                [16. + copy.offset[0], 1. + copy.offset[1]],
                copy.uv_origin,
                copy.uv_size,
                copy.size,
            ));
        }
        for (offset, crop, uv_size, area) in cases {
            let mut q = quad([origin[0] + offset[0], origin[1] + offset[1]], area);
            q.uv_origin = crop;
            q.uv_size = uv_size;
            let mut line = quad([origin[0] + 4., origin[1] + 5.], [161., 1.]);
            line.tint = super::sidebar_text::native_radar_outline_color(
                super::sidebar_chrome::SidebarTheme::Allied,
            );
            let mut cpu = RadarSurface::new(168, 110);
            cpu.paint_quad(&q, origin, &source, source_size);
            cpu.paint_solid(&line, origin);
            let actual = gpu.render(
                &[
                    Layer {
                        quad: q,
                        rgba: source.clone(),
                        size: source_size,
                    },
                    Layer {
                        quad: line,
                        rgba: vec![255; 4],
                        size: [1, 1],
                    },
                ],
                [168, 110],
                origin,
            );
            if origin == [0., 0.] && offset == [16., 1.] {
                assert_eq!(
                    &actual[(5 * 168 + 4) * 4..(5 * 168 + 5) * 4],
                    &[164, 210, 255, 255],
                    "native packed Allied outline after enrolled presentation"
                );
            }
            let differences: Vec<_> = actual
                .chunks_exact(4)
                .zip(cpu.rgba.chunks_exact(4))
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .collect();
            assert!(
                differences.is_empty(),
                "{} differing pixels origin{origin:?}, offset{offset:?}, crop{crop:?}: {:?}",
                differences.len(),
                &differences[..differences.len().min(12)]
            );
        }
    }
}

fn assert_pixels(actual: &[u8], expected: &[u8], label: &str) {
    assert_eq!(actual.len(), expected.len());
    let differences: Vec<_> = actual
        .chunks_exact(4)
        .zip(expected.chunks_exact(4))
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .collect();
    assert!(
        differences.is_empty(),
        "{label}: {} differing pixels: {:?}",
        differences.len(),
        &differences[..differences.len().min(12)]
    );
}

#[test]
#[ignore = "requires original retail archives and a real wgpu adapter"]
fn retail_command_bar_atlas_and_production_append_match_native_capture() {
    use crate::sidebar::{command_bar::CommandBarLayout, gadget_flash::SidebarGadgetState};
    let root = std::env::var_os("RA2_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            crate::util::config::GameConfig::load()
                .unwrap()
                .paths
                .ra2_dir
        });
    let mut assets = crate::assets::asset_manager::AssetManager::new(
        &root,
        crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
    )
    .unwrap();
    assets.load_nested("localmd.mix").unwrap();
    let (art, rgba, size) = super::sidebar_chrome::packed_command_bar_fixture(
        &assets,
        super::sidebar_chrome::SidebarTheme::Allied,
    );
    assert_eq!(art.slots[0], [0, 1, 3, 4, 6, 9].map(Some));
    let canvas =
        |entry: super::sidebar_chrome::SidebarChromeEntry| entry.pixel_size.map(|n| n as u32);
    let layout = CommandBarLayout::new(
        [800, 600],
        168,
        canvas(art.left_cap[0].unwrap()),
        canvas(art.background.unwrap()),
        canvas(art.right_cap.unwrap()),
        true,
    )
    .unwrap();
    let mut instances = Vec::new();
    let camera = [-316.25, 1086.75];
    crate::app::presentation::sidebar_build::command_bar::append_prepared(
        &mut instances,
        &art,
        layout,
        &art.slots[0],
        &SidebarGadgetState::default(),
        camera,
    );
    let layers: Vec<_> = instances
        .into_iter()
        .map(|quad| Layer {
            quad,
            rgba: rgba.clone(),
            size,
        })
        .collect();
    let actual = Gpu::new().render(&layers, [800, 600], camera);
    let expected =
        crate::test_fixture::bytes("tools/sidebar_oracle/command_bar/neutral-bar-rgb565.bin");
    let mut differences = 0;
    for (i, word) in expected.chunks_exact(2).enumerate() {
        let word = u16::from_le_bytes(word.try_into().unwrap());
        let p = ((568 + i / 632) * 800 + i % 632) * 4;
        // The native PCX stores zero-filled RGB565 components (7B05C0),
        // whereas live GPU output uses the independently enrolled presentation
        // codebooks. Compare the same stored word, not unlike expansion stages.
        let actual_word = (u16::from(actual[p] >> 3) << 11)
            | (u16::from(actual[p + 1] >> 2) << 5)
            | u16::from(actual[p + 2] >> 3);
        differences += usize::from(actual_word != word);
    }
    eprintln!(
        "Native command bar: 20224 GPU pixels through PCX RGB565 extraction, {differences} differences"
    );
    assert_eq!(differences, 0);
}

#[test]
#[ignore = "requires a wgpu adapter; checks production retained owner across availability changes"]
fn production_radar_loss_and_reversal_preserve_final_gpu_pixels() {
    use super::radar_anim::{RadarAnimPhase, RadarPresentation};
    let gpu = Gpu::new();
    let origin = [632.25, -413.75];
    let frames: Vec<Vec<u8>> = (0..33)
        .map(|frame| [frame * 5, 24, 64, 255].repeat(168 * 110))
        .collect();
    let mut radar = RadarPresentation::new(frames.clone(), 168, 110).unwrap();
    radar.set_has_radar(true);
    for frame in 0..32 {
        assert!(radar.advance_draw(frame * 64));
    }
    let source: Vec<u8> = (0..140 * 108)
        .flat_map(|i| {
            [
                (i % 256) as u8,
                ((i * 73) % 256) as u8,
                ((i * 191) % 256) as u8,
                255,
            ]
        })
        .collect();
    let housing = quad(origin, [168., 110.]);
    let geometry =
        super::native_radar_surface::NativeRadarSurfaceGeometry::from_raw_rect(30, 20, 300, 180)
            .unwrap();
    let copy = super::minimap_projection::generated_primary_copy_frame(geometry, 140., 108.);
    let mut map = quad(
        [
            origin[0] + 16. + copy.offset[0],
            origin[1] + 1. + copy.offset[1],
        ],
        copy.size,
    );
    map.uv_origin = copy.uv_origin;
    map.uv_size = copy.uv_size;
    let mut line = quad([origin[0] + 20., origin[1] + 8.], [120., 1.]);
    line.tint = super::sidebar_text::native_radar_outline_color(
        super::sidebar_chrome::SidebarTheme::Allied,
    );
    let online = |base: &[u8], source: &[u8], line: SpriteInstance| {
        gpu.render(
            &[
                Layer {
                    quad: housing,
                    rgba: base.to_vec(),
                    size: [168, 110],
                },
                Layer {
                    quad: map,
                    rgba: source.to_vec(),
                    size: [140, 108],
                },
                Layer {
                    quad: line,
                    rgba: vec![255; 4],
                    size: [1, 1],
                },
            ],
            [168, 110],
            origin,
        )
    };
    let retained = |base: &[u8]| {
        gpu.render(
            &[Layer {
                quad: housing,
                rgba: base.to_vec(),
                size: [168, 110],
            }],
            [168, 110],
            origin,
        )
    };
    let displayed = online(&frames[32], &source, line);
    radar.retain_online_content(&source, [140, 108], &[map], [line].into_iter(), origin);
    assert_pixels(&radar.surface.rgba, &displayed, "retained online output");
    radar.set_has_radar(false);
    assert!(radar.advance_draw(2032));
    assert_pixels(&retained(&radar.surface.rgba), &displayed, "pre-due loss");
    let changed_source = [80, 160, 208, 255].repeat(140 * 108);
    radar.retain_online_content(
        &changed_source,
        [140, 108],
        &[map],
        [line].into_iter(),
        origin,
    );
    assert_pixels(
        &retained(&radar.surface.rgba),
        &displayed,
        "internal map changes while closing",
    );
    radar.set_has_radar(true);
    assert!(!radar.advance_draw(2032));
    assert_pixels(
        &retained(&radar.surface.rgba),
        &displayed,
        "pre-due reversal",
    );
    assert!(radar.advance_draw(2048));
    assert_eq!(radar.phase(), RadarAnimPhase::Online);
    assert_pixels(
        &online(&radar.surface.rgba, &source, line),
        &displayed,
        "unchanged final Online image at endpoint",
    );
    line.position[1] += 3.;
    let changed_display = online(&frames[32], &changed_source, line);
    radar.retain_online_content(
        &changed_source,
        [140, 108],
        &[map],
        [line].into_iter(),
        origin,
    );
    radar.set_has_radar(false);
    assert!(radar.advance_draw(2080));
    assert_pixels(
        &retained(&radar.surface.rgba),
        &changed_display,
        "new Online map and outline retained without trails",
    );
    assert!(radar.advance_draw(2112));
    assert_pixels(
        &retained(&radar.surface.rgba),
        &retained(&frames[31]),
        "first due closing frame replaces previous map",
    );
}

#[test]
#[ignore = "requires a wgpu adapter; actual provider encoded tint and final display"]
fn native_radar_outline_all_themes_match_gpu_presentation() {
    let gpu = Gpu::new();
    for (theme, rgba) in [
        (
            super::sidebar_chrome::SidebarTheme::Allied,
            [164, 210, 255, 255],
        ),
        (
            super::sidebar_chrome::SidebarTheme::Soviet,
            [255, 255, 0, 255],
        ),
        (
            super::sidebar_chrome::SidebarTheme::Yuri,
            [255, 255, 0, 255],
        ),
    ] {
        let mut line = quad([0.; 2], [168., 1.]);
        line.tint = super::sidebar_text::native_radar_outline_color(theme);
        let actual = gpu.render(
            &[Layer {
                quad: line,
                rgba: vec![255; 4],
                size: [1, 1],
            }],
            [168, 1],
            [0.; 2],
        );
        assert_pixels(
            &actual,
            &rgba.repeat(168),
            "native packed outline after enrolled presentation",
        );
    }
}

macro_rules! attribute {
    ($location:expr, $format:ident, $($field:tt)+) => {
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::$format,
            offset: offset_of!(SpriteInstance, $($field)+) as u64,
            shader_location: $location,
        }
    };
}

const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 15] = [
    attribute!(0, Float32x2, position),
    attribute!(1, Float32x2, size),
    attribute!(2, Float32x2, uv_origin),
    attribute!(3, Float32x2, uv_size),
    attribute!(4, Float32, depth),
    attribute!(5, Float32x3, tint),
    attribute!(6, Float32, alpha),
    attribute!(7, Uint32, draw_state.remap_row),
    attribute!(8, Uint32, draw_state.fx_flags),
    attribute!(9, Float32x4, draw_state.fx_params),
    attribute!(10, Float32x4, draw_state.effect_tint),
    attribute!(11, Float32, z_adjust),
    attribute!(12, Uint32, z_gradient),
    attribute!(13, Float32x2, zshape_origin),
    attribute!(14, Uint32x4, palette_light),
];
