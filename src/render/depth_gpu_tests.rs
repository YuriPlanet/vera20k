//! Offscreen regression coverage for the actual tactical depth shaders.
//!
//! These tests upload production `SpriteInstance`/`CameraUniform` data and
//! execute the checked-in WGSL. They do not replace shader math with a CPU
//! implementation. Synthetic texels isolate depth admission, transparent holes,
//! BUILDNGZ seed handling, indexed VXL palettes, and tint preservation. A stock
//! cliff case also exercises the production asset decoder and terrain instance
//! builder. These are Rust regressions, not gamemd pixel-parity goldens.
//! Pipeline setup is a minimal offscreen harness, so production draw-list
//! routing still needs its own tests/comparison.
//!
//! Run explicitly on a machine with a wgpu adapter:
//! `cargo test -p vera20k --lib render::depth_gpu_tests:: -- --ignored`

use std::mem::size_of;
use std::time::Duration;

use wgpu::util::DeviceExt;

use super::batch::{CameraUniform, SpriteInstance};
use super::draw_state::DrawState;
use super::frame_readback::PendingBgra8Readback;
use super::native_z::{Z_GRADIENT_ZSHAPE_CLIP_FLAG, Z_GRADIENT_ZSHAPE_FLAG, ZGradient};

const SIDE: u32 = 8;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

#[derive(Clone, Copy, Debug)]
enum Shader {
    Batch,
    Terrain,
    SpriteRead,
    SpriteWrite,
    Voxel,
}

impl Shader {
    fn source(self) -> String {
        use super::tactical_shader::{sprite_source, world_source};
        match self {
            Self::Batch => sprite_source(include_str!("batch_shader.wgsl")),
            Self::Terrain => world_source(include_str!("zdepth_shader.wgsl")),
            Self::SpriteRead | Self::SpriteWrite => {
                sprite_source(include_str!("zsprite_shader.wgsl"))
            }
            Self::Voxel => world_source(include_str!("sprite_voxel_shader.wgsl")),
        }
    }

    fn writes_depth(self) -> bool {
        matches!(self, Self::Terrain | Self::SpriteWrite)
    }
}

#[derive(Clone)]
struct Layer {
    shader: Shader,
    instance: SpriteInstance,
    rgba: Vec<u8>,
    z_bytes: Vec<u8>,
    source_size: [u32; 2],
    indices: Vec<u8>,
    palette_override: Option<Vec<u8>>,
    /// A palette-indexed SHP page: these palette rows take the colour binding
    /// in place of `rgba`, and the instance's `source_palette` picks the row.
    palette_rows: Option<Vec<u8>>,
}

impl Layer {
    fn solid(shader: Shader, rgba: [u8; 4], z_adjust: f32) -> Self {
        Self {
            shader,
            instance: SpriteInstance {
                position: [0.0, 0.0],
                size: [SIDE as f32; 2],
                uv_size: [1.0; 2],
                tint: [1.0; 3],
                alpha: 1.0,
                depth: 0.5,
                z_adjust,
                z_gradient: ZGradient::Flat as u32,
                ..Default::default()
            },
            rgba: rgba.repeat((SIDE * SIDE) as usize),
            z_bytes: vec![0; (SIDE * SIDE) as usize],
            source_size: [SIDE; 2],
            indices: vec![33; (SIDE * SIDE) as usize],
            palette_override: None,
            palette_rows: None,
        }
    }

    fn hole(&mut self, x: u32, y: u32) {
        let pixel = (y * self.source_size[0] + x) as usize;
        self.rgba[pixel * 4 + 3] = 0;
        self.indices[pixel] = 0;
    }
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Gpu {
    fn new() -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: None,
            ..Default::default()
        }))
        .expect("explicit GPU regression test requires a wgpu adapter");
        eprintln!("Depth shader test adapter: {:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Depth shader regression device"),
            ..Default::default()
        }))
        .expect("request offscreen test device");
        Self { device, queue }
    }

    fn texture(
        &self,
        format: wgpu::TextureFormat,
        bytes: &[u8],
        size: [u32; 2],
    ) -> wgpu::TextureView {
        self.device
            .create_texture_with_data(
                &self.queue,
                &wgpu::TextureDescriptor {
                    label: Some("Depth test source"),
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

    fn render(&self, layers: &[Layer]) -> Vec<[u8; 4]> {
        self.render_sized(layers, [SIDE; 2])
    }

    fn render_sized(&self, layers: &[Layer], size: [u32; 2]) -> Vec<[u8; 4]> {
        self.render_sized_zoom(layers, size, 1.0)
    }

    fn render_sized_zoom(&self, layers: &[Layer], size: [u32; 2], zoom: f32) -> Vec<[u8; 4]> {
        let camera = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Depth test camera"),
                contents: bytemuck::bytes_of(&CameraUniform {
                    screen_size: [size[0] as f32, size[1] as f32],
                    camera_pos: [0.0; 2],
                    zoom,
                    world_origin_y: -100.0,
                    world_height: 256.0,
                    _pad: 0.0,

                    native_z_origin_y: 0.0,
                    _native_z_pad: 0.0,
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let (neutral_a, neutral_a_uniform) =
            super::shroud_buffer::neutral_gpu_source(&self.device, &self.queue);
        let sampler = self
            .device
            .create_sampler(&wgpu::SamplerDescriptor::default());
        let color = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth test output"),
            size: extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth test shared Z"),
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            ..color_descriptor(size)
        });
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());

        for (index, layer) in layers.iter().enumerate() {
            let module = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Production tactical shader under test"),
                    source: wgpu::ShaderSource::Wgsl(layer.shader.source().into()),
                });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Offscreen tactical shader harness"),
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some(if matches!(layer.shader, Shader::Batch) {
                            "vs_depth"
                        } else {
                            "vs_main"
                        }),
                        buffers: &[wgpu::VertexBufferLayout {
                            array_stride: size_of::<SpriteInstance>() as u64,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &crate::render::batch::SPRITE_INSTANCE_ATTRIBUTES,
                        }],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some("fs_main"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: FORMAT,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: Default::default(),
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth32Float,
                        depth_write_enabled: layer.shader.writes_depth(),
                        // TMP leaf 0x547CF0 admits equality. Ordinary SHP leaves
                        // 0x494B60/0x497FD0 and building leaves 0x4958D0/0x4990E0
                        // use strict less-than. This exercises those contracts,
                        // not production BatchRenderer's pipeline selection.
                        depth_compare: if matches!(layer.shader, Shader::Terrain) {
                            wgpu::CompareFunction::LessEqual
                        } else {
                            wgpu::CompareFunction::Less
                        },
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    multiview: None,
                    cache: None,
                });
            let camera_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Depth test camera group"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: camera.as_entire_binding(),
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
            let voxel = matches!(layer.shader, Shader::Voxel);
            let color_source = if voxel {
                self.texture(
                    wgpu::TextureFormat::R8Uint,
                    &layer.indices,
                    layer.source_size,
                )
            } else if let Some(rows) = &layer.palette_rows {
                self.texture(
                    wgpu::TextureFormat::Rgba8UnormSrgb,
                    rows,
                    [256, (rows.len() / 1024) as u32],
                )
            } else {
                self.texture(
                    wgpu::TextureFormat::Rgba8UnormSrgb,
                    &layer.rgba,
                    layer.source_size,
                )
            };
            let z_source = self.texture(
                wgpu::TextureFormat::R8Unorm,
                &layer.z_bytes,
                layer.source_size,
            );
            let mut texture_entries = vec![wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&color_source),
            }];
            if !voxel {
                texture_entries.push(wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                });
            }
            if matches!(layer.shader, Shader::Terrain) {
                texture_entries.push(wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&z_source),
                });
            }
            // The actual indexed shader takes non-remap byte 33 from the
            // palette and byte 16 from house row 1. Row 0 is deliberately red.
            // Its declared sampler is unused by textureLoad, so the automatic
            // pipeline layout contains only the two live texture bindings.
            let mut palette_bytes = vec![0; 256 * 4];
            palette_bytes[33 * 4..34 * 4].copy_from_slice(&layer.rgba[..4]);
            if let Some(bytes) = &layer.palette_override {
                palette_bytes.clone_from(bytes);
            }
            let palette = self.texture(
                wgpu::TextureFormat::Rgba8UnormSrgb,
                &palette_bytes,
                [256, 1],
            );
            let ramp_bytes = if layer.palette_override.is_some() {
                palette_bytes[16 * 4..32 * 4].repeat(2)
            } else {
                [RED.repeat(16), GREEN.repeat(16)].concat()
            };
            let house_ramp =
                self.texture(wgpu::TextureFormat::Rgba8UnormSrgb, &ramp_bytes, [16, 2]);
            let voxel_palette_group = voxel.then(|| {
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Depth test indexed VXL palette and house ramp"),
                    layout: &pipeline.get_bind_group_layout(2),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&palette),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&house_ramp),
                        },
                    ],
                })
            });
            // TMP uses its packed depth plane; VXL already uploaded the indexed
            // source at binding 0. Do not manufacture an unused SHP plane from
            // Layer::solid's default dimensions for resized TMP fixtures.
            let index_source = matches!(
                layer.shader,
                Shader::Batch | Shader::SpriteRead | Shader::SpriteWrite
            )
            .then(|| {
                self.texture(
                    wgpu::TextureFormat::R8Uint,
                    &layer.indices,
                    layer.source_size,
                )
            });
            if let Some(index_source) = &index_source {
                texture_entries.push(wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(index_source),
                });
            }
            let texture_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Depth test source group"),
                layout: &pipeline.get_bind_group_layout(1),
                entries: &texture_entries,
            });
            let zshape_group = matches!(layer.shader, Shader::SpriteRead | Shader::SpriteWrite)
                .then(|| {
                    self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("Depth test zshape group"),
                        layout: &pipeline.get_bind_group_layout(2),
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&z_source),
                        }],
                    })
                });
            let instances = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Production SpriteInstance bytes"),
                    contents: bytemuck::bytes_of(&layer.instance),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Depth shader regression draw"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: if index == 0 {
                            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: if index == 0 {
                            wgpu::LoadOp::Clear(super::native_z::STORED_DEPTH_CLEAR)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &camera_group, &[]);
            pass.set_bind_group(1, &texture_group, &[]);
            if let Some(group) = &zshape_group {
                pass.set_bind_group(2, group, &[]);
            }
            if let Some(group) = &voxel_palette_group {
                pass.set_bind_group(2, group, &[]);
            }
            pass.set_vertex_buffer(0, instances.slice(..));
            pass.draw(0..6, 0..1);
        }

        let readback = PendingBgra8Readback::encode(
            &self.device,
            &mut encoder,
            &color,
            FORMAT,
            size[0],
            size[1],
        )
        .expect("encode offscreen readback");
        let submission = self.queue.submit([encoder.finish()]);
        readback
            .finish(&self.device, submission, Duration::from_secs(10))
            .expect("read actual GPU pixels")
            .chunks_exact(4)
            .map(|bgra| [bgra[2], bgra[1], bgra[0], bgra[3]])
            .collect()
    }
}

fn extent(size: [u32; 2]) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: size[0],
        height: size[1],
        depth_or_array_layers: 1,
    }
}

fn color_descriptor(size: [u32; 2]) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: None,
        size: extent(size),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    }
}

fn cliff_depth_lanes() -> Layer {
    let mut terrain = Layer::solid(Shader::Terrain, RED, -31.0);
    // Native TMP ground rows: 31, 30, 19, 18, 10, 9, 8, 31.
    for row in terrain.z_bytes.chunks_exact_mut(SIDE as usize) {
        row.copy_from_slice(&[0, 1, 12, 13, 21, 22, 23, 0]);
    }
    terrain
}

#[test]
#[ignore = "requires a wgpu adapter; run explicitly"]
fn cliff_contributions_change_shp_and_indexed_voxel_admission_at_exact_boundaries() {
    let gpu = Gpu::new();
    // FootClass::GetZAdjustment 0x004DAFC0 contributes +10/+20 on the
    // reached cliff-neighbor branch. These are additions to a deliberately
    // chosen base -20, not complete Foot Z adjustments or native goldens.
    // The vertical gradient's quantization gives row-3 ground rows 30, 18,
    // and 9. Paired TMP lanes straddle each boundary and include equality.
    let cases = [
        (0.0, [RED, RED, BLUE, BLUE, BLUE, BLUE, BLUE, BLUE]),
        (10.0, [RED, RED, RED, RED, BLUE, BLUE, BLUE, BLUE]),
        (20.0, [RED, RED, RED, RED, RED, RED, BLUE, BLUE]),
    ];
    for shader in [Shader::SpriteRead, Shader::Voxel] {
        for (cliff_term, expected) in cases {
            let mut terrain = cliff_depth_lanes();
            terrain.hole(7, 3);
            let mut unit = Layer::solid(shader, BLUE, -20.0 + cliff_term);
            unit.instance.z_gradient = ZGradient::Vertical as u32;
            let pixels = gpu.render(&[terrain, unit]);
            for x in 0..SIDE {
                assert_eq!(
                    pixels[(3 * SIDE + x) as usize],
                    expected[x as usize],
                    "{shader:?}, cliff contribution +{cliff_term}, lane {x}: strict equality, front/behind, or TMP hole changed"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires a wgpu adapter; run explicitly"]
fn indexed_voxel_palette_remap_zero_holes_and_read_only_depth_reach_actual_shader() {
    let gpu = Gpu::new();
    let mut voxel = Layer::solid(Shader::Voxel, BLUE, -12.0);
    voxel.instance.draw_state.remap_row = 1;
    voxel.indices[(3 * SIDE + 4) as usize] = 16;
    voxel.hole(3, 3);
    let pixels = gpu.render(&[Layer::solid(Shader::Terrain, RED, -8.0), voxel]);
    assert_eq!(
        pixels[(3 * SIDE + 3) as usize],
        RED,
        "index zero must discard"
    );
    assert_eq!(
        pixels[(3 * SIDE + 4) as usize],
        GREEN,
        "house ramp row was ignored"
    );
    assert_eq!(
        pixels[(3 * SIDE + 5) as usize],
        BLUE,
        "ordinary palette byte was ignored"
    );

    // VXL_CacheBlit 0x00707480 reaches read-only leaf 0x00497FD0. A
    // nearer indexed draw must not block the later intermediate SHP draw.
    let pixels = gpu.render(&[
        Layer::solid(Shader::Terrain, RED, -8.0),
        Layer::solid(Shader::Voxel, GREEN, -24.0),
        Layer::solid(Shader::SpriteRead, BLUE, -12.0),
    ]);
    assert!(
        pixels.iter().all(|pixel| *pixel == BLUE),
        "indexed VXL wrote shared Z"
    );
}

#[test]
#[ignore = "requires a wgpu adapter; run explicitly"]
fn cliff_voxel_hull_turret_and_barrel_use_the_shared_composite_seed() {
    let gpu = Gpu::new();
    // Native composite cache blit 0x0073B140 seeds one bounding rectangle.
    // Deliberately unequal layer bounds catch accidentally using a turret's
    // or barrel's local height after adding the shared cliff contribution.
    let bounds = [
        ([0.0, 2.0], [8.0, 6.0]),
        ([1.0, 0.0], [6.0, 5.0]),
        ([3.0, 1.0], [2.0, 3.0]),
    ];
    for cliff_term in [10.0, 20.0] {
        let mut full = Layer::solid(Shader::Voxel, BLUE, -20.0 + cliff_term);
        full.instance.z_gradient = ZGradient::Vertical as u32;
        let baseline = gpu.render(&[cliff_depth_lanes(), full.clone()]);
        let mut layers = vec![cliff_depth_lanes()];
        for (position, size) in bounds {
            let mut part = full.clone();
            part.instance.position = position;
            part.instance.size = size;
            part.instance.zshape_origin = [0.0, SIDE as f32];
            layers.push(part);
        }
        let actual = gpu.render(&layers);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let covered = bounds.iter().any(|(position, size)| {
                    x as f32 >= position[0]
                        && (x as f32) < position[0] + size[0]
                        && y as f32 >= position[1]
                        && (y as f32) < position[1] + size[1]
                });
                let pixel = (y * SIDE + x) as usize;
                let expected = if covered { baseline[pixel] } else { RED };
                assert_eq!(
                    actual[pixel], expected,
                    "shared composite +{cliff_term} at ({x},{y})"
                );
            }
        }
        // Sensitivity control: these bounds must visibly distinguish the
        // shared seed from the old per-layer fallback, not compare empty art.
        for layer in &mut layers[1..] {
            layer.instance.zshape_origin = [0.0; 2];
        }
        assert_ne!(
            gpu.render(&layers),
            actual,
            "fixture missed local-seed regression at +{cliff_term}"
        );
    }
}

fn indexed_bridge_test_layer(position: [f32; 2], size: [u32; 2], index: u8) -> Layer {
    let mut layer = Layer::solid(Shader::Voxel, BLUE, 0.0);
    layer.instance.position = position;
    layer.instance.size = [size[0] as f32, size[1] as f32];
    layer.instance.draw_state.remap_row = 1;
    layer.source_size = size;
    layer.indices = vec![index; (size[0] * size[1]) as usize];
    layer.rgba = BLUE.repeat((size[0] * size[1]) as usize);
    layer.z_bytes = vec![0; (size[0] * size[1]) as usize];
    layer
}

#[test]
#[ignore = "requires a wgpu adapter; run explicitly"]
fn bridge_split_shared_parts_match_two_clipped_indexed_composite_blits() {
    use super::native_z::pack_voxel_z_gradient;

    let gpu = Gpu::new();
    let size = [20, 24];
    let mut terrain = Layer::solid(Shader::Terrain, RED, -40.0);
    terrain.source_size = size;
    terrain.instance.size = [20.0, 24.0];
    terrain.rgba = RED.repeat(20 * 24);
    terrain.z_bytes = (0..20 * 24)
        .map(|pixel| 40 - [5, 10, 11, 12, 18, 19, 20, 21, 22, 23][pixel % 10])
        .collect();

    // Native contract: one 20x32 rectangle at y=-4; original split y=12.
    // Tactical clipping leaves upper y=0/h=12 and lower y=12/h=8. These
    // explicitly authored reference rectangles do not call the new helper.
    // Reference texels first compose opaque palette indices, then undergo
    // the two actual production shader blits. This is a Rust GPU regression,
    // not a gamemd image golden or a claim about translucent composition.
    let mut parts = vec![
        indexed_bridge_test_layer([0.0, -2.0], [20, 30], 33),
        indexed_bridge_test_layer([2.0, -4.0], [12, 24], 16),
        indexed_bridge_test_layer([8.0, 3.0], [3, 25], 33),
    ];
    parts[0].hole(0, 8); // uncovered hole at world (0,6)
    parts[1].hole(1, 9); // turret hole reveals hull at world (3,5)
    parts[2].hole(1, 13);

    let mut assembled = indexed_bridge_test_layer([0.0, -4.0], [20, 32], 0);
    for part in &parts {
        for y in 0..part.source_size[1] {
            for x in 0..part.source_size[0] {
                let byte = part.indices[(y * part.source_size[0] + x) as usize];
                if byte != 0 {
                    let dest_x = part.instance.position[0] as i32 + x as i32;
                    let dest_y = part.instance.position[1] as i32 + y as i32 + 4;
                    assembled.indices[(dest_y * 20 + dest_x) as usize] = byte;
                }
            }
        }
    }
    let mut upper = assembled.clone();
    upper.instance.position = [0.0, 0.0];
    upper.instance.size = [20.0, 12.0];
    upper.instance.uv_origin = [0.0, 4.0 / 32.0];
    upper.instance.uv_size = [1.0, 12.0 / 32.0];
    upper.instance.zshape_origin = [0.0, 12.0];
    upper.instance.z_adjust = -5.0;
    upper.instance.z_gradient = ZGradient::Flat as u32;
    let mut lower = assembled;
    lower.instance.position = [0.0, 12.0];
    lower.instance.size = [20.0, 8.0];
    lower.instance.uv_origin = [0.0, 16.0 / 32.0];
    lower.instance.uv_size = [1.0, 8.0 / 32.0];
    lower.instance.zshape_origin = [12.0, 8.0];
    lower.instance.z_gradient = ZGradient::Vertical as u32;
    let expected = gpu.render_sized(&[terrain.clone(), upper, lower], size);

    for part in &mut parts {
        part.instance.z_gradient = pack_voxel_z_gradient(ZGradient::Vertical, true);
        part.instance.zshape_origin = [-4.0, 32.0];
        part.instance.draw_state.fx_params[3] = 20.0;
    }
    let mut draws = vec![terrain];
    draws.extend(parts);
    let actual = gpu.render_sized(&draws, size);
    assert_eq!(
        actual, expected,
        "shared split differs from composite-first two blits"
    );
    // Upper row 6 has native ground row 11; lower row 12 has 22. Terrain
    // lanes straddle these values, so equality must reject, not tie-paint.
    assert_eq!(actual[6 * 20 + 1], BLUE);
    assert_eq!(actual[6 * 20 + 2], RED);
    assert_eq!(actual[6 * 20 + 3], RED);
    assert_eq!(actual[12 * 20 + 7], GREEN);
    assert_eq!(actual[12 * 20 + 8], RED);
    assert_eq!(actual[12 * 20 + 9], RED);
    assert_eq!(actual[12 * 20 + 17], BLUE, "bottom strip lost full width");
    assert_eq!(actual[6 * 20], RED, "index zero did not expose terrain");
    assert!(actual[20 * 20..].iter().all(|pixel| *pixel == RED));

    let mut control = draws.clone();
    for part in &mut control[1..] {
        part.instance.z_gradient = ZGradient::Vertical as u32;
    }
    assert_ne!(
        gpu.render_sized(&control, size),
        actual,
        "fixture missed absent split"
    );
    for part in &mut control[1..] {
        part.instance.z_gradient = pack_voxel_z_gradient(ZGradient::Vertical, true);
        part.instance.zshape_origin = [part.instance.position[1], part.instance.size[1]];
    }
    assert_ne!(
        gpu.render_sized(&control, size),
        actual,
        "fixture missed per-part boundary"
    );
    for part in &mut control[1..] {
        part.instance.zshape_origin = [-4.0, 32.0];
        part.instance.draw_state.fx_params[3] = 0.0;
    }
    assert_ne!(
        gpu.render_sized(&control, size),
        actual,
        "fixture missed clipped seed/height"
    );
}

#[test]
#[ignore = "requires a wgpu adapter; run explicitly"]
fn bridge_split_height_threshold_and_zoom_keep_original_quad_sampling() {
    use super::native_z::pack_voxel_z_gradient;

    let gpu = Gpu::new();
    let mut unit = indexed_bridge_test_layer([2.0, 2.0], [20, 16], 33);
    unit.instance.zshape_origin = [2.0, 16.0];
    unit.instance.z_gradient = pack_voxel_z_gradient(ZGradient::Moderate, false);
    let mut terrain = Layer::solid(Shader::Terrain, RED, -10.0);
    terrain.instance.size = [30.0, 40.0];
    let ordinary = gpu.render_sized(&[terrain.clone(), unit.clone()], [30, 40]);
    unit.instance.z_gradient = pack_voxel_z_gradient(ZGradient::Moderate, true);
    assert_eq!(
        gpu.render_sized(&[terrain.clone(), unit], [30, 40]),
        ordinary,
        "height16 must retain fallback gradient"
    );
    let mut seventeen = indexed_bridge_test_layer([2.0, 2.0], [20, 17], 33);
    seventeen.instance.zshape_origin = [2.0, 17.0];
    seventeen.instance.z_gradient = pack_voxel_z_gradient(ZGradient::Moderate, false);
    let ordinary = gpu.render_sized(&[terrain.clone(), seventeen.clone()], [30, 40]);
    seventeen.instance.z_gradient = pack_voxel_z_gradient(ZGradient::Moderate, true);
    assert_ne!(
        gpu.render_sized(&[terrain, seventeen], [30, 40]),
        ordinary,
        "height17 must select the two regions"
    );

    // With clear depth, every opaque texel passes both gradients. Toggling
    // the split must leave full-quad UV sampling unchanged at non-native
    // zoom: no new padded/cropped edge at the internal 16-row boundary.
    let mut tall = indexed_bridge_test_layer([2.0, 2.0], [20, 28], 33);
    tall.instance.zshape_origin = [2.0, 28.0];
    for y in 0..28 {
        for x in 0..20 {
            tall.indices[(y * 20 + x) as usize] = if (x + y) % 5 == 0 {
                0
            } else if y % 2 == 0 {
                16
            } else {
                33
            };
        }
    }
    let size = [48, 64];
    for zoom in [0.75, 1.25, 2.0] {
        tall.instance.z_gradient = pack_voxel_z_gradient(ZGradient::Vertical, false);
        let ordinary = gpu.render_sized_zoom(&[tall.clone()], size, zoom);
        tall.instance.z_gradient = pack_voxel_z_gradient(ZGradient::Vertical, true);
        let split = gpu.render_sized_zoom(&[tall.clone()], size, zoom);
        let mut mismatches = split
            .iter()
            .zip(&ordinary)
            .enumerate()
            .filter(|(_, (split, ordinary))| split != ordinary)
            .map(|(index, (split, ordinary))| {
                (
                    index % size[0] as usize,
                    index / size[0] as usize,
                    *split,
                    *ordinary,
                )
            });
        let first = mismatches.next();
        let count = usize::from(first.is_some()) + mismatches.count();
        assert_eq!(
            count, 0,
            "split changed palette sampling at zoom {zoom}; first (x,y,split,ordinary)={first:?}"
        );
    }
}

#[test]
#[ignore = "requires a wgpu adapter; run explicitly"]
fn bridge_direct_shp_unit_uses_whole_body_flat_fudge_even_below_split_height() {
    // Stock SQD reaches direct SHP 0x73CE0D, not composite 0x73B140.
    // The caller's a7=-16 adds to FootZ and DrawSHP's -2 wrapper term;
    // the caller also selects gradient0 for the entire frame, with no h>16
    // test. Isolate that production-shader transport on an eight-row frame.
    let gpu = Gpu::new();
    let mut ordinary = Layer::solid(Shader::SpriteRead, BLUE, -2.0);
    ordinary.instance.z_gradient = ZGradient::Vertical as u32;
    let baseline = gpu.render(&[cliff_depth_lanes(), ordinary.clone()]);
    ordinary.instance.z_adjust -= 16.0;
    ordinary.instance.z_gradient = ZGradient::Flat as u32;
    let actual = gpu.render(&[cliff_depth_lanes(), ordinary]);
    assert_eq!(
        &actual[3 * SIDE as usize..4 * SIDE as usize],
        &[RED, RED, BLUE, BLUE, BLUE, BLUE, BLUE, RED]
    );
    assert_ne!(
        actual, baseline,
        "fixture missed whole-body direct-SHP branch"
    );
}

#[test]
#[ignore = "requires installed stock YR assets and a wgpu adapter; run explicitly"]
fn stock_cliff_decoder_and_terrain_instances_occlude_shp_and_indexed_voxels() {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
    use crate::map::terrain::{TerrainCell, TerrainGrid, TilePlacement};
    use crate::map::theater::{TileKey, load_theater, load_tile_images};

    let asset_root = std::env::var_os("RA2_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            crate::util::config::GameConfig::load()
                .expect("stock test requires RA2_DIR or config.toml")
                .paths
                .ra2_dir
        });
    let mut assets = AssetManager::new(&asset_root, MediaArchiveMode::STOCK_DIGITAL)
        .expect("load installed YR archive stack");
    let theater = load_theater(&mut assets, "TEMPERATE").expect("activate stock temperate assets");
    let tile_id = (0..theater.lookup.len())
        .find(|index| {
            theater
                .lookup
                .filename(*index as i32)
                .is_some_and(|name| name.eq_ignore_ascii_case("cliff01.tem"))
        })
        .expect("stock theater must resolve cliff01.tem") as u16;
    let key = TileKey {
        tile_id,
        sub_tile: 2,
        variant: 0,
    };
    let mut images = load_tile_images(
        &assets,
        &theater.lookup,
        &theater.iso_palette,
        &HashSet::from([key]),
    );
    let tile = images
        .remove(&key)
        .expect("production decoder must load cliff01.tem sub-tile 2");
    // Retail fixture inspected 2026-09-08: ra2.mix -> isotemp.mix,
    // CLI entry 0x37FBE2D9, 14456-byte TMP. This case loads through the
    // runtime theater search, not the tool's catalog-only fallback.
    assert_eq!(
        (tile.width, tile.height, tile.offset_x, tile.offset_y),
        (60, 60, 0, -30)
    );
    let samples = [(30u32, 53u32, 6u8), (36, 47, 12), (58, 29, 15)];
    for (x, y, depth) in samples {
        let pixel = (y * tile.width + x) as usize;
        assert_eq!(
            tile.depth[pixel], depth,
            "stock cliff depth fixture at ({x},{y}) changed"
        );
        assert_eq!(tile.rgba[pixel * 4 + 3], 255, "stock sample must be opaque");
    }
    assert_eq!(
        tile.rgba[3], 0,
        "stock tile corner must remain a transparent hole"
    );
    let placement = TilePlacement {
        uv_origin: [0.0; 2],
        uv_size: [1.0; 2],
        pixel_size: [tile.width as f32, tile.height as f32],
        draw_offset: [tile.offset_x as f32, tile.offset_y as f32],
    };
    let grid = TerrainGrid {
        cells: vec![TerrainCell {
            screen_x: 0.0,
            screen_y: 30.0,
            tile_id,
            sub_tile: 2,
            z: 0,
            rx: 1,
            ry: 1,
            is_water: false,
            variant: 0,
            tint: [1.0; 3],
            radar_left: [0; 3],
            radar_right: [0; 3],
            has_damaged_data: false,
        }],
        world_width: 60.0,
        world_height: 256.0,
        origin_x: 0.0,
        origin_y: -100.0,
        local_bounds: None,
        bridge_middle_tiles: None,
    };
    let lookup = |id, sub, variant| {
        assert_eq!((id, sub, variant), (tile_id, 2, 0));
        Some(placement)
    };
    let mut instances = super::terrain_instances::build_visible_instances(
        &grid,
        None,
        0.0,
        0.0,
        60.0,
        60.0,
        Some(&lookup),
        None,
    )
    .normal;
    assert_eq!(instances.len(), 1);
    let mut cliff = Layer::solid(Shader::Terrain, RED, 0.0);
    cliff.instance = instances.remove(0);
    cliff.rgba = tile.rgba;
    cliff.z_bytes = tile.depth;
    cliff.source_size = [tile.width, tile.height];

    let gpu = Gpu::new();
    let background = gpu.render_sized(std::slice::from_ref(&cliff), [60; 2]);
    // A 60-row vertical sprite with base -1, plus the isolated cliff term.
    // At the three retail sample pixels, expected visibility is respectively
    // [front, behind, behind], [front, front, behind], [front, front, front].
    // Expected colors come from the actual terrain readback, preserving its
    // stock palette rather than substituting a colored depth-only rectangle.
    for shader in [Shader::SpriteRead, Shader::Voxel] {
        for (cliff_term, admitted) in [
            (0.0, [true, true, true]),
            (10.0, [false, true, true]),
            (20.0, [false, false, true]),
        ] {
            let mut unit = Layer::solid(shader, BLUE, -1.0 + cliff_term);
            unit.instance.size = [60.0; 2];
            unit.instance.z_gradient = ZGradient::Vertical as u32;
            let pixels = gpu.render_sized(&[cliff.clone(), unit], [60; 2]);
            for ((x, y, _), visible) in samples.into_iter().zip(admitted) {
                let pixel = (y * 60 + x) as usize;
                assert_ne!(
                    background[pixel], BLUE,
                    "stock color must distinguish unit admission"
                );
                let expected = if visible { BLUE } else { background[pixel] };
                assert_eq!(
                    pixels[pixel], expected,
                    "stock cliff {shader:?} +{cliff_term} at ({x},{y})"
                );
            }
            assert_eq!(
                pixels[0], BLUE,
                "stock cliff hole wrote depth for {shader:?} +{cliff_term}"
            );
        }
    }
}

#[test]
#[ignore = "requires a wgpu adapter; run explicitly"]
fn terrain_and_building_depth_hide_only_behind_pixels_and_preserve_holes() {
    let gpu = Gpu::new();
    // The terrain depth here represents native ground row 8; flat SHP with
    // z_adjust=0 is behind it. This fixture is deliberately below the depth
    // axis clamp and samples interior pixels (no raster-edge ambiguity).
    let mut terrain = Layer::solid(Shader::Terrain, RED, -16.0);
    terrain.z_bytes.fill(8);
    // The far lane represents row 0, allowing the sprite through. This makes
    // admission depend on the sampled TMP Z byte, not just instance depth.
    for y in 0..SIDE {
        terrain.z_bytes[(y * SIDE + 5) as usize] = 16;
    }
    terrain.hole(3, 3);
    let behind = Layer::solid(Shader::SpriteRead, BLUE, 0.0);
    let pixels = gpu.render(&[terrain, behind]);
    assert_eq!(
        pixels[(3 * SIDE + 3) as usize],
        BLUE,
        "terrain alpha hole wrote depth"
    );
    assert_eq!(
        pixels[(3 * SIDE + 4) as usize],
        RED,
        "behind sprite passed terrain Z"
    );
    assert_eq!(
        pixels[(3 * SIDE + 5) as usize],
        BLUE,
        "TMP per-pixel depth byte was ignored"
    );

    // The high-bridge atlas stores row bytes with the opposite sign from TMP.
    let mut bridge = Layer::solid(Shader::Terrain, RED, 0.0);
    bridge.z_bytes.fill(8);
    bridge.instance.draw_state.fx_params[3] = -1.0;
    let pixels = gpu.render(&[bridge, Layer::solid(Shader::SpriteRead, BLUE, 0.0)]);
    assert!(
        pixels.iter().all(|pixel| *pixel == RED),
        "bridge row atlas sign failed to occlude the behind sprite"
    );

    // A nearer read-only sprite may change color, but must not prevent a
    // later, intermediate-depth sprite from drawing (native unit blitters
    // 0x494B60/0x497FD0 test existing Z without writing it).
    let pixels = gpu.render(&[
        Layer::solid(Shader::Terrain, RED, -8.0),
        Layer::solid(Shader::SpriteRead, GREEN, -24.0),
        Layer::solid(Shader::SpriteRead, BLUE, -12.0),
    ]);
    assert!(
        pixels.iter().all(|pixel| *pixel == BLUE),
        "read-only sprite wrote Z or front sprite was rejected"
    );

    let mut building = Layer::solid(Shader::SpriteWrite, GREEN, -24.0);
    building.hole(3, 3);
    let pixels = gpu.render(&[
        Layer::solid(Shader::Terrain, RED, -8.0),
        building,
        Layer::solid(Shader::SpriteRead, BLUE, -12.0),
    ]);
    assert_eq!(
        pixels[(3 * SIDE + 3) as usize],
        BLUE,
        "building alpha hole wrote depth"
    );
    assert_eq!(
        pixels[(3 * SIDE + 4) as usize],
        GREEN,
        "building did not write Z"
    );

    // Equality differs between TMP and SHP: terrain can replace equal Z,
    // while a sprite at the same native row must fail the strict test.
    let pixels = gpu.render(&[
        Layer::solid(Shader::Terrain, RED, -8.0),
        Layer::solid(Shader::Terrain, GREEN, -8.0),
        Layer::solid(Shader::SpriteRead, BLUE, -5.0),
    ]);
    assert_eq!(
        pixels[(3 * SIDE + 3) as usize],
        GREEN,
        "equal sprite depth must fail after equal terrain depth passes"
    );
}

#[test]
#[ignore = "requires a wgpu adapter; run explicitly"]
fn building_zshape_uses_constant_raw_seed_and_signed_shape_bytes() {
    let gpu = Gpu::new();
    // CC_Draw_Shape 0x437C39..0x437C72 / 0x437E67..0x437EA7:
    // BUILDNGZ takes the raw seed DEFAULT_Z-height-screen_top+1+z_adjust,
    // then subtracts the signed shape byte. It does not walk the gradient.
    // Eight rows at top 0/height 8/z_adjust 0 have a constant ground row 7.
    let mut building = Layer::solid(Shader::SpriteWrite, BLUE, 0.0);
    building.instance.z_gradient = ZGradient::Vertical as u32 | Z_GRADIENT_ZSHAPE_FLAG;
    building.z_bytes.fill(128);
    for y in 0..SIDE {
        building.z_bytes[(y * SIDE + 1) as usize] = 129;
        building.z_bytes[(y * SIDE + 6) as usize] = 127;
    }
    let pixels = gpu.render(&[Layer::solid(Shader::Terrain, RED, -7.0), building]);
    for y in 0..SIDE {
        assert_eq!(
            pixels[(y * SIDE + 1) as usize],
            BLUE,
            "+1 zshape lane must be nearer at every row {y}"
        );
        assert_eq!(
            pixels[(y * SIDE + 3) as usize],
            RED,
            "neutral zshape lane must fail equality at every row {y}"
        );
        assert_eq!(
            pixels[(y * SIDE + 6) as usize],
            RED,
            "-1 zshape lane must be farther at every row {y}"
        );
    }
}

#[test]
#[ignore = "requires a wgpu adapter; run explicitly"]
fn second_shape_clips_color_and_depth_and_seeds_from_intersected_bottom() {
    let gpu = Gpu::new();
    // A shape at (2,-2) intersects the 8x8 stored body at x=2..7, y=0..5.
    // These are synthetic regression fixtures for the binary-derived shape
    // intersection contract, not native-executed parity goldens.
    let clipped = |extended: bool| {
        let mut body = Layer::solid(Shader::SpriteWrite, BLUE, 0.0);
        body.instance.zshape_origin = [2.0, -2.0];
        body.instance.z_gradient = ZGradient::Vertical as u32
            | Z_GRADIENT_ZSHAPE_CLIP_FLAG
            | if extended { Z_GRADIENT_ZSHAPE_FLAG } else { 0 };
        body.z_bytes.fill(128);
        body
    };

    // The intersected bottom is row 6, giving extended raw-seed ground row 5.
    // At equality only the signed +1 lane (shape x=1 -> world x=3) can pass.
    let mut body = clipped(true);
    for y in 0..SIDE {
        body.z_bytes[(y * SIDE + 1) as usize] = 129;
    }
    let pixels = gpu.render(&[Layer::solid(Shader::Terrain, RED, -5.0), body]);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let expected = if x == 3 && y < 6 { BLUE } else { RED };
            assert_eq!(
                pixels[(y * SIDE + x) as usize],
                expected,
                "extended shape intersection/seed at ({x},{y})"
            );
        }
    }

    // Raw SHP uses the ordinary vertical gradient after intersection. For
    // the six-row rect, the first three rows have ground row 6, the next
    // three row 5. Shape bytes must not be subtracted on this dispatch.
    let mut raw = clipped(false);
    raw.z_bytes.fill(255);
    let pixels = gpu.render(&[Layer::solid(Shader::Terrain, RED, -5.0), raw]);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let expected = if x >= 2 && y < 3 { BLUE } else { RED };
            assert_eq!(
                pixels[(y * SIDE + x) as usize],
                expected,
                "raw frame must clip, keep ordinary gradient, and ignore shape byte at ({x},{y})"
            );
        }
    }

    // Probe depth separately: an intermediate-depth terrain draw replaces
    // the background outside the shape, but cannot replace its opaque body.
    // A discarded color fragment must not leave an invisible depth blocker.
    let pixels = gpu.render(&[
        Layer::solid(Shader::Terrain, RED, 0.0),
        clipped(true),
        Layer::solid(Shader::Terrain, GREEN, -4.0),
    ]);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let expected = if x >= 2 && y < 6 { BLUE } else { GREEN };
            assert_eq!(
                pixels[(y * SIDE + x) as usize],
                expected,
                "shape-outside fragment wrote depth at ({x},{y})"
            );
        }
    }
}

#[test]
#[ignore = "requires a wgpu adapter; run explicitly"]
fn depth_sprite_preserves_batch_tint_and_opacity() {
    let gpu = Gpu::new();
    let mut ordinary = Layer::solid(Shader::Batch, [120, 160, 200, 255], -8.0);
    ordinary.instance.tint = [0.5, 1.3, 0.75];
    ordinary.instance.alpha = 0.75;
    ordinary.instance.draw_state = DrawState {
        fx_params: [0.6, 0.0, 1.0, 0.0],
        ..Default::default()
    };
    let baseline = gpu.render(std::slice::from_ref(&ordinary));
    ordinary.shader = Shader::SpriteRead;
    let depth_read = gpu.render(std::slice::from_ref(&ordinary));
    ordinary.shader = Shader::SpriteWrite;
    let depth_write = gpu.render(std::slice::from_ref(&ordinary));
    for (index, expected) in baseline.iter().enumerate() {
        for actual in [depth_read[index], depth_write[index]] {
            for channel in 0..4 {
                assert!(
                    actual[channel].abs_diff(expected[channel]) <= 1,
                    "depth pipeline changed batch color/opacity at pixel {index}: {actual:?} != {expected:?}"
                );
            }
        }
    }
    // Ensure the test isn't vacuously comparing empty images.
    assert!(
        baseline
            .iter()
            .all(|pixel| pixel[0] > 10 && pixel[1] > 10 && pixel[2] > 10)
    );
}

/// Native goldens contain no retail art: original gamemd palette/scanline
/// instructions were executed over a synthetic palette covering every channel
/// byte. Every N27/N53 row and 240..254 mask lane reaches the production WGSL
/// entrypoint with production vertex ABI, sRGB textures and framebuffer writes.
#[test]
#[ignore = "requires a wgpu adapter; run explicitly"]
fn native_palette_tables_match_all_ordinary_shader_pixels() {
    use super::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION;
    use super::palette_light::{PaletteLight, native_fixtures, native_row};
    let gpu = Gpu::new();
    let rgba: Vec<u8> = (0..=255u8)
        .flat_map(|i| [i, i.wrapping_mul(73), 255 - i, if i == 0 { 0 } else { 255 }])
        .collect();
    for fixture in native_fixtures() {
        for shader in [
            Shader::Batch,
            Shader::Terrain,
            Shader::SpriteRead,
            Shader::SpriteWrite,
            Shader::Voxel,
        ] {
            // TMP owns the ordinary mask; house exemption belongs to SHP/VXL.
            if fixture.house && matches!(shader, Shader::Terrain) {
                continue;
            }
            let layers: Vec<_> = (0..fixture.rows)
                .map(|row| {
                    let brightness = (0..=2000)
                        .find(|b| native_row(*b, 127, fixture.rows) == row)
                        .unwrap();
                    let mut layer = Layer::solid(shader, RED, 0.0);
                    layer.source_size = [256, 1];
                    layer.rgba.clone_from(&rgba);
                    layer.z_bytes = vec![0; 256];
                    layer.indices = (0..=255u8).collect();
                    layer.palette_override = Some(rgba.clone());
                    layer.instance.position = [0.0, row as f32];
                    layer.instance.size = [256.0, 1.0];
                    layer.instance.palette_light = if fixture.plain {
                        PaletteLight::plain(fixture.rows, brightness)
                    } else {
                        PaletteLight::new(fixture.rgb, fixture.rows, brightness, fixture.house)
                    };
                    layer
                })
                .collect();
            let actual = gpu.render_sized(&layers, [256, fixture.rows]);
            for (pixel, (got, bytes)) in
                actual.iter().zip(fixture.bytes.chunks_exact(2)).enumerate()
            {
                let word = if pixel % 256 == 0 {
                    0
                } else {
                    u16::from_le_bytes(bytes.try_into().unwrap())
                };
                let stored = [
                    ((word >> 11) as u8) << 3,
                    (((word >> 5) & 63) as u8) << 2,
                    ((word & 31) as u8) << 3,
                    255,
                ];
                let wanted = ACTIVE_RETAIL_RGB565_PRESENTATION.quantize_rgba8(stored);
                assert_eq!(
                    *got,
                    wanted,
                    "{shader:?}, rgb={:?}, N={}, mask={}, row={}, index={}",
                    fixture.rgb,
                    fixture.rows,
                    fixture.house,
                    pixel / 256,
                    pixel % 256
                );
            }
        }
    }
}

/// The colour word a draw's [`super::palette_light::PaletteLight`] carries
/// reaches every ordinary shader's pixels ORed into the native RGB565 word
/// after the conversion, as the tinted blitters OR it (`0x00494CD9`,
/// `0x00498293`, `0x004993C2`; `tools/superweapon_oracle.py` `blitters`),
/// and never reaches a source hole.
#[test]
#[ignore = "requires a wgpu adapter; run explicitly"]
fn colour_word_ors_into_every_ordinary_shader_pixel() {
    use super::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION;
    use super::palette_light::{PaletteLight, native_fixtures, native_row};
    let gpu = Gpu::new();
    let rgba: Vec<u8> = (0..=255u8)
        .flat_map(|i| [i, i.wrapping_mul(73), 255 - i, if i == 0 { 0 } else { 255 }])
        .collect();
    let fixture = native_fixtures()
        .into_iter()
        .find(|f| f.house && f.rows == 53)
        .unwrap();
    let row = 26;
    let brightness = (0..=2000)
        .find(|b| native_row(*b, 127, fixture.rows) == row)
        .unwrap();
    for colour in [0x0018u16, 0xE000, 0xE798] {
        for shader in [
            Shader::Batch,
            Shader::SpriteRead,
            Shader::SpriteWrite,
            Shader::Voxel,
        ] {
            let mut layer = Layer::solid(shader, RED, 0.0);
            layer.source_size = [256, 1];
            layer.rgba.clone_from(&rgba);
            layer.z_bytes = vec![0; 256];
            layer.indices = (0..=255u8).collect();
            layer.palette_override = Some(rgba.clone());
            layer.instance.size = [256.0, 1.0];
            layer.instance.palette_light =
                PaletteLight::new(fixture.rgb, fixture.rows, brightness, fixture.house)
                    .with_colour_word(colour);
            let actual = gpu.render_sized(&[layer], [256, 1]);
            for (index, got) in actual.iter().enumerate() {
                let offset = (row as usize * 256 + index) * 2;
                let word = if index == 0 {
                    0
                } else {
                    u16::from_le_bytes(fixture.bytes[offset..offset + 2].try_into().unwrap())
                        | colour
                };
                let stored = [
                    ((word >> 11) as u8) << 3,
                    (((word >> 5) & 63) as u8) << 2,
                    ((word & 31) as u8) << 3,
                    255,
                ];
                assert_eq!(
                    *got,
                    ACTIVE_RETAIL_RGB565_PRESENTATION.quantize_rgba8(stored),
                    "{shader:?}, colour {colour:#06x}, index {index}"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires a GPU adapter"]
fn native_shp_loader_atlas_palette_indices_reach_production_pixels() {
    let gpu = Gpu::new();
    let probe = super::sprite_atlas::native_palette_probe_page();
    let fixture = super::palette_light::native_fixtures()
        .into_iter()
        .find(|f| f.house && f.rows == 53 && f.rgb == [992, 768, 512])
        .unwrap();
    let profile = super::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION;
    for shader in [Shader::Batch, Shader::SpriteRead, Shader::SpriteWrite] {
        let mut layer = Layer::solid(shader, RED, 0.0);
        layer.palette_rows = Some(probe.palette_row.clone());
        layer.instance.source_palette = 1;
        layer.indices = probe.indices.clone();
        layer.source_size = probe.size;
        layer.z_bytes = vec![0; (probe.size[0] * probe.size[1]) as usize];
        layer.instance.size = [256.0, 1.0];
        layer.instance.uv_origin = [3.0 / 262.0, 1.0 / 3.0];
        layer.instance.uv_size = [256.0 / 262.0, 1.0 / 3.0];
        layer.instance.palette_light =
            super::palette_light::PaletteLight::new(fixture.rgb, 53, 1000, true);
        let actual = gpu.render_sized(&[layer], [256, 1]);
        for (i, got) in actual.iter().enumerate() {
            let offset = (26 * 256 + i) * 2;
            let word = if i == 0 {
                0
            } else {
                u16::from_le_bytes(fixture.bytes[offset..offset + 2].try_into().unwrap())
            };
            let expected = [
                profile.five_bit[((word >> 11) & 31) as usize],
                profile.six_bit[((word >> 5) & 63) as usize],
                profile.five_bit[(word & 31) as usize],
                255,
            ];
            assert_eq!(*got, expected, "{shader:?}, source index {i}");
        }
    }
}

#[test]
#[ignore = "requires a GPU adapter"]
fn indexed_shp_pages_draw_the_pixels_their_rgba_pages_drew() {
    // The probe page through each SHP shader: once from the RGBA that
    // `ShpFile::frame_to_rgba` converts its frame to, once from its indices
    // and palette row. The row sits behind a decoy row, so the row selection
    // is exercised. The precomposed path and a native palette conversion must
    // both agree pixel for pixel, holes included.
    let gpu = Gpu::new();
    let probe = super::sprite_atlas::native_palette_probe_page();
    let rows = [vec![255; 1024], probe.palette_row.clone()].concat();
    let fixture = super::palette_light::native_fixtures()
        .into_iter()
        .find(|f| f.house && f.rows == 53 && f.rgb == [992, 768, 512])
        .unwrap();
    for shader in [Shader::Batch, Shader::SpriteRead, Shader::SpriteWrite] {
        for palette_light in [
            super::palette_light::PaletteLight::default(),
            super::palette_light::PaletteLight::new(fixture.rgb, 53, 1000, true),
        ] {
            let mut rgba = Layer::solid(shader, RED, 0.0);
            rgba.rgba = probe.rgba.clone();
            rgba.indices = probe.indices.clone();
            rgba.source_size = probe.size;
            rgba.z_bytes = vec![0; (probe.size[0] * probe.size[1]) as usize];
            rgba.instance.size = probe.size.map(|v| v as f32);
            rgba.instance.palette_light = palette_light;
            let mut indexed = rgba.clone();
            indexed.palette_rows = Some(rows.clone());
            indexed.instance.source_palette = 2;
            assert_eq!(
                gpu.render_sized(&[indexed], probe.size),
                gpu.render_sized(&[rgba], probe.size),
                "{shader:?}, {palette_light:?}"
            );
        }
    }
}

/// Offscreen consumer seam for app-layer tests of production lighting inputs.
/// Uses the ordinary terrain WGSL and real TMP pixels; no CPU color substitute.
pub(crate) fn render_terrain_lighting_probe(
    tile: &crate::map::theater::TileImage,
    instance: SpriteInstance,
) -> Vec<[u8; 4]> {
    let layer = Layer {
        shader: Shader::Terrain,
        instance,
        rgba: tile.rgba.clone(),
        z_bytes: tile.depth.clone(),
        source_size: [tile.width, tile.height],
        indices: Vec::new(),
        palette_override: None,
        palette_rows: None,
    };
    Gpu::new().render_sized(&[layer], [tile.width, tile.height])
}

#[test]
#[ignore = "requires a wgpu adapter; run explicitly"]
fn bunker_front_wall_interleaves_with_indexed_voxel_without_writing_z() {
    use super::tactical_draw_plan::{RenderZPolicy, SpriteEncoding};
    use crate::app::presentation::render::draw_plan_lowering::{
        NativeDisplayOrder, ObjectPieceInstance, ObjectTexture, PlannedObjectInstance,
        lower_ground_object_instances,
    };
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_slot_replacement.display.json",
    ))
    .unwrap();
    let ids = [11, 20, 10]; // back wall, tank, front wall; deliberately not id order.
    let ordered: Vec<_> = native["orders"][0]["order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| ids[value.as_u64().unwrap() as usize])
        .collect();
    let order = NativeDisplayOrder::new(&ordered);
    let gpu = Gpu::new();
    // Synthetic texels and seed isolate the production GPU's read-only
    // SHP/VXL compositing. These are not stock NATBNK/LTNK pixel goldens.
    let back = Layer::solid(Shader::SpriteRead, RED, -12.0);
    let tank = Layer::solid(Shader::Voxel, BLUE, -24.0);
    let mut front = Layer::solid(Shader::SpriteRead, GREEN, -12.0);
    for y in 0..SIDE / 2 {
        for x in 0..SIDE {
            front.hole(x, y);
        }
    }
    let layers = [back, tank, front];
    let planned: Vec<_> = [2usize, 0, 1]
        .into_iter()
        .map(|index| {
            let encoding = if index == 1 {
                SpriteEncoding::Voxel
            } else {
                SpriteEncoding::Plain
            };
            PlannedObjectInstance::object(
                order.object_draw(ids[index], encoding).unwrap(),
                vec![ObjectPieceInstance {
                    target: if index == 1 {
                        ObjectTexture::UnitAtlasPage(0)
                    } else {
                        ObjectTexture::ShpPage(index)
                    },
                    render_z: RenderZPolicy::ReadOnly,
                    instance: layers[index].instance,
                }],
            )
        })
        .collect();
    let lowered = lower_ground_object_instances(planned);
    assert_eq!(lowered.owners, ordered);
    let submitted: Vec<_> = lowered
        .owners
        .iter()
        .map(|id| layers[ids.iter().position(|candidate| candidate == id).unwrap()].clone())
        .collect();
    let pixels = gpu.render(&submitted);
    for y in 0..SIDE {
        for x in 0..SIDE {
            assert_eq!(
                pixels[(y * SIDE + x) as usize],
                if y < SIDE / 2 { BLUE } else { GREEN },
                "wall must cover hull only where its source texel exists: ({x},{y})"
            );
        }
    }
    let wrong = gpu.render(&[layers[0].clone(), layers[2].clone(), layers[1].clone()]);
    assert!(
        wrong.iter().all(|pixel| *pixel == BLUE),
        "negative control: submitting both walls before the tank reproduces the uncovered hull"
    );
}
