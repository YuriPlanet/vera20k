//! Native transient tactical combat-light presentation.
//!
//! gamemd does not create an animation, particle, or map light for an
//! IronCurtain/ForceShield impact. It appends a small record to a dedicated
//! vector and later edits the already-composed 16-bit tactical surface. This
//! renderer keeps that presentation state outside the simulation, builds the
//! native 64 byte-mask surfaces once, and applies every active record in the
//! native reverse insertion order in encoded RGB565 space.

use wgpu::util::DeviceExt;

/// One combat light the tactical pass draws this frame. Render-owned draw DTO
/// (F06); the app runtime that ages and drains lights produces these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CombatLightDrawRecord {
    pub coord: crate::sim::projectile::ProjectileCoord,
    pub surface_index: i32,
    pub flags: u32,
}
use crate::render::batch::CameraUniform;
use crate::render::gpu::GpuContext;

const SHADER: &str = include_str!("combat_light.wgsl");
const MASK_WIDTH: u32 = 256;
const MASK_HEIGHT: u32 = 128;
const NORMAL_MASK_LAYER_COUNT: u32 = 64;
const MASK_LAYER_COUNT: u32 = 74;
const INITIAL_CAPACITY: usize = 16;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuLightInstance {
    center: [i32; 2],
    surface_index: u32,
    flags: u32,
}

struct CompositionTargets {
    texture: wgpu::Texture,
    render_view: wgpu::TextureView,
    encoded_view: wgpu::TextureView,
    scratch_texture: wgpu::Texture,
    // Retained with the bind group to make GPU resource ownership explicit.
    _scratch_encoded_view: wgpu::TextureView,
    scene_bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

/// App-owned renderer for the native persistent combat-light vector.
pub(crate) struct CombatLightRenderer {
    pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    mask_bind_group: wgpu::BindGroup,
    mask_texture: wgpu::Texture,
    spotlight_radius: Option<i32>,
    scene_bind_group_layout: wgpu::BindGroupLayout,
    instance_buffer: wgpu::Buffer,
    capacity: usize,
    /// Frame-local logical record -> uploaded instance. Unsupported native
    /// indices remain holes rather than shifting subsequent Display commands.
    prepared_indices: Vec<Option<u32>>,
    instances: Vec<GpuLightInstance>,
    /// Conservative target-space bounds aligned with the uploaded instances.
    /// Rebuilt with the camera, resolution and logical map on every prepare.
    copy_bounds: Vec<[u32; 4]>,
    surface_format: wgpu::TextureFormat,
    encoded_format: wgpu::TextureFormat,
    targets: CompositionTargets,
}

impl CombatLightRenderer {
    pub(crate) fn new(gpu: &GpuContext) -> Self {
        Self::new_with_device(&gpu.device, &gpu.queue, gpu.surface_format)
    }

    pub(crate) fn new_with_device(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let encoded_format = super::webgl::encoded_format(device, surface_format);
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Combat Light Camera Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Combat Light Camera"),
            contents: &super::webgl::uniform_bytes(
                device,
                bytemuck::bytes_of(&CameraUniform {
                    screen_size: [1.0, 1.0],
                    camera_pos: [0.0, 0.0],
                    zoom: 1.0,
                    world_origin_y: 0.0,
                    world_height: 1.0,
                    _pad: 0.0,

                    native_z_origin_y: 0.0,
                    _native_z_pad: 0.0,
                }),
            ),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Combat Light Camera Bind Group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let scene_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Combat Light Scene Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }],
            });
        let mask_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Combat Light Native Mask Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let mask_texture = create_native_mask_texture(device, queue);
        let mask_view = mask_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("Combat Light Native Mask Array View"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let mask_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Combat Light Native Mask Bind Group"),
            layout: &mask_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&mask_view),
            }],
        });

        let source =
            super::webgl::camera_source(device, shader_source(super::gpu::webgl_fallback(device)));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Combat Light Exact RGB565 Shader"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Combat Light Exact RGB565 Pipeline Layout"),
            bind_group_layouts: &[
                Some(&camera_layout),
                Some(&scene_bind_group_layout),
                Some(&mask_layout),
            ],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Sint32x2, 1 => Uint32, 2 => Uint32];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Combat Light Exact RGB565 Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GpuLightInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: encoded_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });

        let instance_buffer = allocate_instance_buffer(device, INITIAL_CAPACITY);
        let targets = create_composition_targets(
            device,
            &scene_bind_group_layout,
            surface_format,
            encoded_format,
            1,
            1,
        );
        Self {
            pipeline,
            camera_buffer,
            camera_bind_group,
            mask_bind_group,
            mask_texture,
            spotlight_radius: None,
            scene_bind_group_layout,
            instance_buffer,
            capacity: INITIAL_CAPACITY,
            prepared_indices: Vec::new(),
            instances: Vec::new(),
            copy_bounds: Vec::new(),
            surface_format,
            encoded_format,
            targets,
        }
    }

    /// Initialize5FF5AA..5FF727 owns surfaces64..73, driven by Rules+7A8.
    /// This renderer keeps one cache keyed by the effective retained radius.
    pub(crate) fn set_spotlight_radius(&mut self, queue: &wgpu::Queue, radius: i32) {
        if self.spotlight_radius == Some(radius) {
            return;
        }
        let mut masks = Vec::with_capacity((MASK_WIDTH * MASK_HEIGHT * 10) as usize);
        for stage in 80..=89 {
            masks.extend(
                crate::render::building_light::generate_type16_mask(radius, stage).unwrap(),
            );
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.mask_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: 64 },
                aspect: wgpu::TextureAspect::All,
            },
            &masks,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(MASK_WIDTH),
                rows_per_image: Some(MASK_HEIGHT),
            },
            wgpu::Extent3d {
                width: MASK_WIDTH,
                height: MASK_HEIGHT,
                depth_or_array_layers: 10,
            },
        );
        self.spotlight_radius = Some(radius);
    }

    pub(crate) fn prepare(
        &mut self,
        gpu: &GpuContext,
        records: &[CombatLightDrawRecord],
        screen_size: [f32; 2],
        camera_pos: [f32; 2],
        zoom: f32,
    ) {
        self.prepare_with_device(
            &gpu.device,
            &gpu.queue,
            records,
            screen_size,
            camera_pos,
            zoom,
        );
    }

    /// Prepare one tactical frame. The composition surface is recreated only
    /// when the render resolution changes.
    pub(crate) fn prepare_with_device(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        records: &[CombatLightDrawRecord],
        screen_size: [f32; 2],
        camera_pos: [f32; 2],
        zoom: f32,
    ) {
        let width = screen_size[0].max(1.0) as u32;
        let height = screen_size[1].max(1.0) as u32;
        if self.targets.width != width || self.targets.height != height {
            self.targets = create_composition_targets(
                device,
                &self.scene_bind_group_layout,
                self.surface_format,
                self.encoded_format,
                width,
                height,
            );
        }
        let camera = CameraUniform {
            screen_size,
            camera_pos,
            zoom,
            world_origin_y: 0.0,
            world_height: 1.0,
            _pad: 0.0,

            native_z_origin_y: 0.0,
            _native_z_pad: 0.0,
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera));
        self.prepared_indices.clear();
        self.instances.clear();
        self.copy_bounds.clear();
        for record in records {
            // Original5FF850 indexes64 surfaces without a clamp. Preserve a
            // missing logical slot for unsupported authored indices; a later
            // retained System or persistent light still addresses its own row.
            if !(0..MASK_LAYER_COUNT as i32).contains(&record.surface_index) {
                self.prepared_indices.push(None);
                continue;
            }
            self.prepared_indices
                .push(Some(self.instances.len() as u32));
            let instance = GpuLightInstance {
                center: project_combat_light_anchor(record.coord),
                surface_index: record.surface_index as u32,
                flags: record.flags,
            };
            self.copy_bounds
                .push(light_copy_bounds(instance.center, &camera, [width, height]));
            self.instances.push(instance);
        }
        if self.instances.is_empty() {
            return;
        }
        if self.instances.len() > self.capacity {
            self.capacity = self.instances.len().next_power_of_two();
            self.instance_buffer = allocate_instance_buffer(device, self.capacity);
        }
        // Upload the entire immutable frame before any draw is encoded.
        // Queue writes precede the next submit; rewriting a single slot
        // between parent draws would make all of them read the last value.
        queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(&self.instances),
        );
    }

    pub(crate) fn composition_view(&self) -> wgpu::TextureView {
        self.targets.render_view.clone()
    }

    /// Live world composition behind [`Self::composition_view`]. Wheel zoom
    /// resolves this into a separate frame before screen-fixed UI is added.
    pub(crate) fn composition_texture(&self) -> &wgpu::Texture {
        &self.targets.texture
    }

    /// Spotlight5FF850 edits this one prepared record's current destination.
    /// The caller retains its Display slot or later persistent-vector order,
    /// and accounts each closed pass through the shared submission owner.
    pub(crate) fn draw_record(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        tactical: [u32; 4],
        record: usize,
    ) -> bool {
        let Some(index) = *self
            .prepared_indices
            .get(record)
            .expect("light draw references this frame's prepared record")
        else {
            return false;
        };
        let [left, top, right, bottom] = self.copy_bounds[index as usize];
        let left = left.max(tactical[0]);
        let top = top.max(tactical[1]);
        let right = right.min(tactical[0].saturating_add(tactical[2]));
        let bottom = bottom.min(tactical[1].saturating_add(tactical[3]));
        if left < right && top < bottom {
            // The shader loads only this fragment's global destination texel,
            // without filtering or neighbours. Keep both origins identical:
            // stale scratch outside the quad/scissor cannot be sampled. wgpu27
            // permits partial copies for these single-sample RGBA/BGRA textures:
            // https://github.com/gfx-rs/wgpu/blob/d61dc3af8856f0cd9023c5e888b791d1b7118559/wgpu-core/src/command/transfer.rs#L489-L574
            let origin = wgpu::Origin3d {
                x: left,
                y: top,
                z: 0,
            };
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.targets.texture,
                    mip_level: 0,
                    origin,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &self.targets.scratch_texture,
                    mip_level: 0,
                    origin,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: right - left,
                    height: bottom - top,
                    depth_or_array_layers: 1,
                },
            );
        }
        // Even an offscreen supported record retains its ordered closed pass
        // and the caller's PassSubmission accounting; only its copy is empty.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Combat Light Native Surface Edit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.targets.encoded_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, &self.targets.scene_bind_group, &[]);
        pass.set_bind_group(2, &self.mask_bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        pass.set_scissor_rect(tactical[0], tactical[1], tactical[2], tactical[3]);
        pass.draw(0..6, index..index + 1);
        true
    }
}

/// Bounds of combat_light.wgsl's uploaded quad, after its existing integer
/// world projection. Zoom moves the centre; the mask remains 256x128 pixels.
fn light_copy_bounds(center: [i32; 2], camera: &CameraUniform, target: [u32; 2]) -> [u32; 4] {
    let full = [0, 0, target[0], target[1]];
    // Restrict this optimisation to normal finite presentation inputs. The
    // integer centre converts exactly here, and screen_size equals the actual
    // viewport. Keep the existing full copy for other domains rather than
    // inferring the shader's extreme-value/overflow behaviour.
    const LIMIT: f32 = 65_536.0;
    if target.iter().any(|&extent| extent > LIMIT as u32)
        || camera.screen_size != target.map(|extent| extent as f32)
        || center
            .iter()
            .any(|&value| value.unsigned_abs() > LIMIT as u32)
        || camera
            .camera_pos
            .iter()
            .any(|value| !value.is_finite() || value.abs() > LIMIT)
        || !(1.0 / 16.0..=16.0).contains(&camera.zoom)
    {
        return full;
    }
    let mut bounds = full;
    for axis in 0..2 {
        let center = f64::from(center[axis]);
        let camera_pos = f64::from(camera.camera_pos[axis]);
        let zoom = f64::from(camera.zoom);
        let half = f64::from([MASK_WIDTH, MASK_HEIGHT][axis]) / 2.0;
        let extent = f64::from(target[axis]);
        let screen_center = (center - camera_pos) * zoom;
        // Outward rounding includes a conservative error budget for the small
        // f32 vertex expression (including reassociation and clip/viewport
        // conversion), plus two pixels of raster margin. This is derived from
        // operand magnitude, not fitted to mask pixels. WGSL allows 2.5 ULP
        // division and reassociation; 64 epsilons covers those operations in
        // the bounded domain above:
        // https://www.w3.org/TR/WGSL/#floating-point-accuracy
        // https://www.w3.org/TR/WGSL/#floating-point-reassociation
        let magnitude = (center.abs() + camera_pos.abs()) * zoom + half + extent;
        let margin = magnitude * f64::from(f32::EPSILON) * 64.0 + 2.0;
        // All operands are finite and bounded. Clamp before converting, so no
        // non-finite/out-of-range float-to-integer cast enters copy commands.
        bounds[axis] = (screen_center - half - margin).floor().clamp(0.0, extent) as u32;
        bounds[axis + 2] = (screen_center + half + margin).ceil().clamp(0.0, extent) as u32;
    }
    bounds
}

/// Native `CoordsToClient2` projects the planar terms with signed integer
/// division, so both axes truncate toward zero before camera and presentation
/// scaling. VERA's common world-row bias remains after that native projection
/// so this direct-surface primitive aligns with the rest of the tactical scene.
fn project_combat_light_anchor(coord: crate::sim::projectile::ProjectileCoord) -> [i32; 2] {
    let (planar_x, planar_y) =
        crate::render::tactical_compat::project_native_planar(coord.x, coord.y);
    [
        planar_x,
        planar_y
            .wrapping_sub(crate::util::native_x87::adjust_for_z_standard(coord.z))
            .wrapping_add(15),
    ]
}

fn allocate_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Combat Light Instances"),
        size: (capacity * std::mem::size_of::<GpuLightInstance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The WebGL2 fallback reads and writes encoded bytes through sRGB views
/// (see `render::webgl`).
pub(crate) fn shader_source(webgl: bool) -> String {
    if webgl {
        super::webgl::encoded_shader(
            SHADER,
            &[
                (
                    "textureLoad(scene, vec2i(input.position.xy), 0)",
                    "fallback_load_encoded(textureLoad(scene, vec2i(input.position.xy), 0))",
                ),
                (
                    "return vec4f(transformed) / 255.0;",
                    "return fallback_store_encoded(vec4f(transformed) / 255.0);",
                ),
            ],
        )
    } else {
        SHADER.to_owned()
    }
}

fn create_composition_targets(
    device: &wgpu::Device,
    scene_layout: &wgpu::BindGroupLayout,
    surface_format: wgpu::TextureFormat,
    encoded_format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> CompositionTargets {
    let extent = wgpu::Extent3d {
        width: width.max(1),
        height: height.max(1),
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Combat Light Tactical Composition Surface"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: surface_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[encoded_format],
    });
    let render_view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("Combat Light Tactical sRGB Render View"),
        format: Some(surface_format),
        usage: Some(wgpu::TextureUsages::RENDER_ATTACHMENT),
        ..Default::default()
    });
    let encoded_view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("Combat Light Tactical Encoded View"),
        format: Some(encoded_format),
        usage: Some(wgpu::TextureUsages::RENDER_ATTACHMENT),
        ..Default::default()
    });
    let scratch_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Combat Light Destination Snapshot"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: surface_format,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[encoded_format],
    });
    let scratch_encoded_view = scratch_texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("Combat Light Destination Encoded Sampling View"),
        format: Some(encoded_format),
        usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
        ..Default::default()
    });
    let scene_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Combat Light Destination Snapshot Bind Group"),
        layout: scene_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&scratch_encoded_view),
        }],
    });
    CompositionTargets {
        texture,
        render_view,
        encoded_view,
        scratch_texture,
        _scratch_encoded_view: scratch_encoded_view,
        scene_bind_group,
        width: extent.width,
        height: extent.height,
    }
}

fn create_native_mask_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Combat Light Native 64-Surface Mask Array"),
        size: wgpu::Extent3d {
            width: MASK_WIDTH,
            height: MASK_HEIGHT,
            depth_or_array_layers: MASK_LAYER_COUNT,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Uint,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let masks = build_native_mask_atlas();
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &masks,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(MASK_WIDTH),
            rows_per_image: Some(MASK_HEIGHT),
        },
        wgpu::Extent3d {
            width: MASK_WIDTH,
            height: MASK_HEIGHT,
            depth_or_array_layers: NORMAL_MASK_LAYER_COUNT,
        },
    );
    texture
}

/// Build BSurface indices 0..63: draw odd-radius concentric filled circles on
/// a 256x256 byte surface, then retain every other source scanline.
fn build_native_mask_atlas() -> Vec<u8> {
    const SOURCE_HEIGHT: usize = 256;
    let mut atlas =
        vec![0; MASK_WIDTH as usize * MASK_HEIGHT as usize * NORMAL_MASK_LAYER_COUNT as usize];
    let mut source = vec![0; MASK_WIDTH as usize * SOURCE_HEIGHT];
    let layer_stride = MASK_WIDTH as usize * MASK_HEIGHT as usize;
    for layer in 0..NORMAL_MASK_LAYER_COUNT as usize {
        source.fill(0);
        let mut radius = (layer as i32) * 2 + 1;
        let mut intensity: u8 = 12;
        while radius > 0 {
            fill_midpoint_circle(&mut source, radius, intensity);
            radius -= 2;
            intensity = intensity.saturating_add(4);
        }
        let layer_start = layer * layer_stride;
        for destination_y in 0..MASK_HEIGHT as usize {
            let source_start = destination_y * 2 * MASK_WIDTH as usize;
            let destination_start = layer_start + destination_y * MASK_WIDTH as usize;
            atlas[destination_start..destination_start + MASK_WIDTH as usize]
                .copy_from_slice(&source[source_start..source_start + MASK_WIDTH as usize]);
        }
    }
    atlas
}

pub(crate) fn fill_midpoint_circle(surface: &mut [u8], radius: i32, intensity: u8) {
    let mut x = 0;
    let mut y = radius;
    let mut decision = 3 - radius * 2;
    while y >= x {
        draw_span(surface, 128 + x, 128 - y, 128 + y, intensity);
        draw_span(surface, 128 + y, 128 - x, 128 + x, intensity);
        draw_span(surface, 128 - x, 128 - y, 128 + y, intensity);
        draw_span(surface, 128 - y, 128 - x, 128 + x, intensity);
        if decision < 0 {
            decision += 4 * x + 6;
        } else {
            decision += 4 * (x - y) + 10;
            y -= 1;
        }
        x += 1;
    }
}

fn draw_span(surface: &mut [u8], y: i32, x0: i32, x1: i32, intensity: u8) {
    // Original7BA610 clips the Circle7BB920's spans against the byte surface.
    // Normal masks fit; authored SpotlightRadius can exceed that domain.
    if !(0..256).contains(&y) {
        return;
    }
    let left = x0.min(x1).max(0);
    let right = x0.max(x1).min(255);
    if left <= right {
        surface[y as usize * 256 + left as usize..=y as usize * 256 + right as usize]
            .fill(intensity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welding_reached_masks_match_original_construction() {
        let native = crate::render::tactical_compat::welding_tests::native();
        let atlas = build_native_mask_atlas();
        let stride = (MASK_WIDTH * MASK_HEIGHT) as usize;
        let rows = native["light_masks"].as_array().unwrap();
        assert!(!rows.is_empty());
        for row in rows {
            let index = row["index"].as_u64().unwrap() as usize;
            assert_eq!(row["size"], serde_json::json!([MASK_WIDTH, MASK_HEIGHT]));
            let expected =
                crate::render::tactical_compat::welding_tests::decode_hex(&row["bytes_hex"]);
            assert_eq!(
                &atlas[index * stride..(index + 1) * stride],
                expected,
                "original5FF420 Welding mask{index}"
            );
        }
    }

    #[test]
    #[ignore = "requires GPU; whole original immediate Welding light pixels and filtered logical index mapping"]
    fn production_welding_light_pixels_and_logical_indices_match_original() {
        use crate::render::tactical_compat::welding_tests as fixture;
        use crate::render::terrain_draw_gpu_tests::{Gpu, encoded};
        let gpu = Gpu::new();
        for format in [
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ] {
            let mut renderer =
                CombatLightRenderer::new_with_device(&gpu.device, &gpu.queue, format);
            for row in fixture::native()["system_draw_cases"].as_array().unwrap() {
                let input = &row["input"];
                let light = &row["light"];
                let size = fixture::array::<2>(&input["size"]).map(|value| value as u32);
                let mut camera = fixture::array::<2>(&input["camera"]);
                camera[1] += crate::util::lepton::absolute_leptons_to_screen(0, 0, 0).1 as i32;
                let at = fixture::array::<3>(&input["position"]);
                let mut records = Vec::new();
                if light["admitted"] == true {
                    let record = CombatLightDrawRecord {
                        coord: crate::sim::projectile::ProjectileCoord::new(at[0], at[1], at[2]),
                        surface_index: fixture::int(&light["surface_index"]),
                        flags: fixture::int(&light["flags"]) as u32,
                    };
                    records.extend([
                        CombatLightDrawRecord {
                            surface_index: -1,
                            ..record
                        },
                        record,
                        CombatLightDrawRecord {
                            surface_index: 74,
                            ..record
                        },
                        CombatLightDrawRecord {
                            coord: crate::sim::projectile::ProjectileCoord::new(
                                at[0] + 4096,
                                at[1],
                                at[2],
                            ),
                            ..record
                        },
                    ]);
                }
                renderer.prepare_with_device(
                    &gpu.device,
                    &gpu.queue,
                    &records,
                    size.map(|value| value as f32),
                    camera.map(|value| value as f32),
                    1.,
                );
                let initial = encoded(fixture::int(&input["background"]) as u16, format)
                    .repeat((size[0] * size[1]) as usize);
                gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: renderer.composition_texture(),
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
                    renderer.composition_texture().size(),
                );
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                if !records.is_empty() {
                    let clip = fixture::array::<4>(&input["clip"]).map(|value| value as u32);
                    assert!(!renderer.draw_record(&mut encoder, clip, 0));
                    assert!(renderer.draw_record(&mut encoder, clip, 1));
                    assert!(!renderer.draw_record(&mut encoder, clip, 2));
                }
                let read = gpu.read(&mut encoder, renderer.composition_texture());
                let output = gpu.finish(encoder, &[read], size);
                let expected = fixture::expected_words(row);
                for (index, (pixel, &expected)) in output[0]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(&expected)
                    .enumerate()
                {
                    let rgb = if format == wgpu::TextureFormat::Bgra8UnormSrgb {
                        [pixel[2], pixel[1], pixel[0]]
                    } else {
                        [pixel[0], pixel[1], pixel[2]]
                    };
                    assert_eq!(
                        crate::render::native_surface_format::RGB565.pack_rgb8(rgb),
                        expected,
                        "{} {format:?} pixel{index}",
                        input["name"]
                    );
                    assert_eq!(pixel[3], 255);
                }
            }
        }
    }

    #[test]
    #[ignore = "requires GPU; bounded destination copies against full-current scratch through the same light raster"]
    fn bounded_light_copies_match_full_current_scratch() {
        use crate::render::terrain_draw_gpu_tests::{Gpu, encoded};

        let gpu = Gpu::new();
        let base = CombatLightDrawRecord {
            coord: crate::sim::projectile::ProjectileCoord::new(1024, 768, 128),
            surface_index: 63,
            flags: 1,
        };
        let anchor = project_combat_light_anchor(base.coord).map(|value| value as f32);
        let cases = [
            (
                "fractional",
                [512_u32, 256],
                [180.375_f32, 125.625],
                1.25_f32,
                [7, 5, 490, 240],
                true,
            ),
            (
                "negative_partial",
                [512, 256],
                [-40.25, 8.125],
                0.75,
                [0, 0, 120, 90],
                true,
            ),
            (
                "resized_partial",
                [333, 219],
                [330.625, 209.375],
                2.375,
                [19, 11, 314, 208],
                true,
            ),
            (
                "offscreen",
                [333, 219],
                [-400.25, -200.375],
                0.625,
                [0, 0, 333, 219],
                false,
            ),
            (
                "scissored_overlap",
                [512, 256],
                [200.125, 130.875],
                1.875,
                [85, 30, 190, 150],
                true,
            ),
            // An unusual but finite zoom exercises the conservative fallback.
            (
                "fallback",
                [512, 256],
                [180.375, 125.625],
                1.0 / 32.0,
                [7, 5, 490, 240],
                true,
            ),
        ];
        for format in [
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ] {
            // Reuse both renderers across camera/zoom/record changes and a
            // resize, so bounds must be invalidated by the actual prepare.
            let mut renderers = std::array::from_fn::<_, 2, _>(|_| {
                CombatLightRenderer::new_with_device(&gpu.device, &gpu.queue, format)
            });
            for (case, &(name, size, screen_center, zoom, tactical, visible)) in
                cases.iter().enumerate()
            {
                let camera = std::array::from_fn(|axis| anchor[axis] - screen_center[axis] / zoom);
                let mut records = vec![
                    CombatLightDrawRecord {
                        surface_index: -1,
                        ..base
                    },
                    base,
                    CombatLightDrawRecord {
                        coord: crate::sim::projectile::ProjectileCoord::new(1152, 768, 128),
                        flags: 0,
                        ..base
                    },
                    CombatLightDrawRecord {
                        surface_index: 74,
                        ..base
                    },
                    CombatLightDrawRecord {
                        coord: crate::sim::projectile::ProjectileCoord::new(1024, 832, 128),
                        surface_index: 24,
                        flags: 6,
                    },
                    CombatLightDrawRecord {
                        coord: crate::sim::projectile::ProjectileCoord::new(33792, 768, 128),
                        ..base
                    },
                ];
                if case % 2 == 1 {
                    records.pop();
                    records.rotate_left(1);
                }
                let initial: Vec<u8> = (0..size[1])
                    .flat_map(|y| {
                        (0..size[0]).flat_map(move |x| {
                            let word = 0x5269_u16.wrapping_add(((x * 17 + y * 31) & 0x0fff) as u16);
                            let mut pixel = encoded(word, format);
                            pixel[3] = (128 + ((x + y) & 127)) as u8;
                            pixel
                        })
                    })
                    .collect();
                let stale = [251_u8, 3, 157, 37].repeat((size[0] * size[1]) as usize);
                for renderer in &mut renderers {
                    renderer.prepare_with_device(
                        &gpu.device,
                        &gpu.queue,
                        &records,
                        size.map(|value| value as f32),
                        camera,
                        zoom,
                    );
                    for (texture, bytes) in [
                        (renderer.composition_texture(), &initial),
                        (&renderer.targets.scratch_texture, &stale),
                    ] {
                        gpu.queue.write_texture(
                            wgpu::TexelCopyTextureInfo {
                                texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d::ZERO,
                                aspect: wgpu::TextureAspect::All,
                            },
                            bytes,
                            wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(size[0] * 4),
                                rows_per_image: Some(size[1]),
                            },
                            texture.size(),
                        );
                    }
                }
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                let mut reads = Vec::new();
                for (record, light) in records.iter().enumerate() {
                    for (variant, renderer) in renderers.iter().enumerate() {
                        if variant == 1 {
                            // Transport reference only: seed the entire current
                            // destination immediately before the SAME production
                            // draw. No second raster, mask, blend or geometry
                            // implementation supplies an expected pixel.
                            encoder.copy_texture_to_texture(
                                wgpu::TexelCopyTextureInfo {
                                    texture: renderer.composition_texture(),
                                    mip_level: 0,
                                    origin: wgpu::Origin3d::ZERO,
                                    aspect: wgpu::TextureAspect::All,
                                },
                                wgpu::TexelCopyTextureInfo {
                                    texture: &renderer.targets.scratch_texture,
                                    mip_level: 0,
                                    origin: wgpu::Origin3d::ZERO,
                                    aspect: wgpu::TextureAspect::All,
                                },
                                renderer.composition_texture().size(),
                            );
                        }
                        assert_eq!(
                            renderer.draw_record(&mut encoder, tactical, record),
                            (0..64).contains(&light.surface_index),
                            "{name} {format:?} logical{record}"
                        );
                        reads.push(gpu.read(&mut encoder, renderer.composition_texture()));
                    }
                }
                let output = gpu.finish(encoder, &reads, size);
                let mut previous = &initial;
                let mut changed_draws = 0;
                for (record, pair) in output.chunks_exact(2).enumerate() {
                    assert_eq!(
                        pair[0].iter().zip(&pair[1]).position(|(a, b)| a != b),
                        None,
                        "{name} {format:?} logical{record}: bounded/full-scratch byte"
                    );
                    changed_draws += usize::from(previous != &pair[1]);
                    previous = &pair[1];
                }
                if visible {
                    assert!(
                        changed_draws >= 2,
                        "{name} must exercise sequential overlapping edits"
                    );
                } else {
                    assert_eq!(
                        changed_draws, 0,
                        "offscreen records retain their closed passes"
                    );
                }
            }
        }
    }

    fn electric_native() -> &'static serde_json::Value {
        static CORPUS: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
        CORPUS.get_or_init(|| {
            serde_json::from_str(crate::test_fixture::text(
                "tools/procedural_drawing_oracle/electric_bolt.json",
            ))
            .unwrap()
        })
    }

    #[test]
    fn spark_mask_prefix_matches_original_surface_construction() {
        let atlas = build_native_mask_atlas();
        let stride = (MASK_WIDTH * MASK_HEIGHT) as usize;
        for row in electric_native()["light_draw_cases"].as_array().unwrap() {
            for mask in row["masks"].as_array().unwrap() {
                let index = mask["index"].as_u64().unwrap() as usize;
                assert_eq!(mask["size"], serde_json::json!([MASK_WIDTH, MASK_HEIGHT]));
                let expected: Vec<u8> = mask["bytes_hex"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                    .collect();
                let actual = &atlas[index * stride..(index + 1) * stride];
                let different: Vec<_> = actual
                    .iter()
                    .zip(&expected)
                    .enumerate()
                    .filter(|(_, (actual, expected))| actual != expected)
                    .collect();
                assert!(
                    different.is_empty(),
                    "original5FF420 mask{index}: {} bytes differ, first {:?}",
                    different.len(),
                    &different[..different.len().min(8)]
                );
                assert_eq!(actual.len(), expected.len());
            }
        }
    }

    #[test]
    #[ignore = "requires GPU; original Spotlight5FF850 stock Spark masks and all ten draw stages"]
    fn production_spark_light_pixels_match_original_software_surface() {
        use crate::render::native_surface_format::RGB565;
        use crate::render::terrain_draw_gpu_tests::{Gpu, encoded};

        let gpu = Gpu::new();
        let native = electric_native();
        let mut failures = Vec::new();
        for format in [
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ] {
            let mut renderer =
                CombatLightRenderer::new_with_device(&gpu.device, &gpu.queue, format);
            for row in native["light_draw_cases"].as_array().unwrap() {
                let input = &row["input"];
                let name = input["name"].as_str().unwrap();
                let size: [u32; 2] = input["size"].as_array().map_or([160, 120], |value| {
                    std::array::from_fn(|axis| value[axis].as_u64().unwrap() as u32)
                });
                let camera: [i32; 2] = std::array::from_fn(|axis| {
                    input["camera"][axis].as_i64().unwrap() as i32 + if axis == 1 { 15 } else { 0 }
                });
                let at: [i32; 3] =
                    std::array::from_fn(|axis| row["position"][axis].as_i64().unwrap() as i32);
                let clip: [u32; 4] = input["clip"]
                    .as_array()
                    .map_or([0, 0, size[0], size[1]], |value| {
                        std::array::from_fn(|axis| value[axis].as_u64().unwrap() as u32)
                    });
                let background = input["background"].as_u64().unwrap() as u16;
                let initial = encoded(background, format).repeat((size[0] * size[1]) as usize);
                for draw in row["draws"].as_array().unwrap() {
                    // Original5FF850 supplied this bank index. App lifetime
                    // and FPS admission have their own native boundary tests.
                    let index = native["light_indices"]["rows"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|index| {
                            index["size"] == draw["size"] && index["stage"] == draw["stage"]
                        })
                        .unwrap()["index"]
                        .as_i64()
                        .unwrap() as i32;
                    renderer.prepare_with_device(
                        &gpu.device,
                        &gpu.queue,
                        &[CombatLightDrawRecord {
                            coord: crate::sim::projectile::ProjectileCoord::new(
                                at[0], at[1], at[2],
                            ),
                            surface_index: index,
                            flags: draw["flags"].as_u64().unwrap() as u32,
                        }],
                        size.map(|value| value as f32),
                        camera.map(|value| value as f32),
                        1.,
                    );
                    gpu.queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: renderer.composition_texture(),
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
                        renderer.composition_texture().size(),
                    );
                    let mut encoder = gpu.device.create_command_encoder(&Default::default());
                    assert!(renderer.draw_record(&mut encoder, clip, 0));
                    let read = gpu.read(&mut encoder, renderer.composition_texture());
                    let output = gpu.finish(encoder, &[read], size);
                    let mut expected = vec![background; (size[0] * size[1]) as usize];
                    for pixel in draw["pixels"].as_array().unwrap() {
                        let offset = pixel[1].as_u64().unwrap() as usize * size[0] as usize
                            + pixel[0].as_u64().unwrap() as usize;
                        expected[offset] = pixel[2].as_u64().unwrap() as u16;
                    }
                    // Native goldens are packed RGB565. Residual: this shader
                    // zero-fills low bits; display expansion adds up to 7/3/7
                    // RGB8 levels. Compare the complete original word domain.
                    assert_eq!(output[0].len(), expected.len() * 4);
                    let different: Vec<_> = output[0]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(&expected)
                        .enumerate()
                        .filter_map(|(offset, (actual, &expected))| {
                            let rgb = if format == wgpu::TextureFormat::Bgra8UnormSrgb {
                                [actual[2], actual[1], actual[0]]
                            } else {
                                [actual[0], actual[1], actual[2]]
                            };
                            let word = RGB565.pack_rgb8(rgb);
                            (word != expected || actual[3] != 255)
                                .then_some((offset, word, expected, actual[3]))
                        })
                        .collect();
                    if !different.is_empty() {
                        failures.push(format!(
                            "{name} stage{} {format:?}: {} pixels differ, first {:?}",
                            draw["stage"],
                            different.len(),
                            &different[..different.len().min(8)]
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    #[ignore = "requires GPU; whole original type16 Spotlight5FF850 pixels at stages80 and89"]
    fn production_type16_spotlight_pixels_match_original() {
        use crate::render::native_surface_format::RGB565;
        use crate::render::terrain_draw_gpu_tests::{Gpu, encoded};

        let native = crate::test_fixture::json("tools/procedural_drawing_oracle/spotlight.json");
        let rows = native["type16_draws"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        let gpu = Gpu::new();
        for format in [
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ] {
            let mut renderer =
                CombatLightRenderer::new_with_device(&gpu.device, &gpu.queue, format);
            for row in rows {
                let size: [u32; 2] = serde_json::from_value(row["size"].clone()).unwrap();
                let mut camera: [i32; 2] = serde_json::from_value(row["camera"].clone()).unwrap();
                camera[1] += crate::util::lepton::absolute_leptons_to_screen(0, 0, 0).1 as i32;
                let at: [i32; 3] = serde_json::from_value(row["position"].clone()).unwrap();
                let clip: [u32; 4] = serde_json::from_value(row["clip"].clone()).unwrap();
                let background = row["background"].as_u64().unwrap() as u16;
                let surface_index = row["surface_index"].as_i64().unwrap() as i32;
                assert!((64..=73).contains(&surface_index));
                for invariant in [
                    "rng_unchanged",
                    "z_unchanged",
                    "alpha_unchanged",
                    "guards_unchanged",
                    "text_unchanged",
                ] {
                    assert_eq!(row[invariant], true, "native {invariant}");
                }
                renderer.set_spotlight_radius(&gpu.queue, row["radius"].as_i64().unwrap() as i32);
                renderer.prepare_with_device(
                    &gpu.device,
                    &gpu.queue,
                    &[CombatLightDrawRecord {
                        coord: crate::sim::projectile::ProjectileCoord::new(at[0], at[1], at[2]),
                        surface_index,
                        flags: row["flags"].as_u64().unwrap() as u32,
                    }],
                    size.map(|value| value as f32),
                    camera.map(|value| value as f32),
                    1.,
                );
                let initial = encoded(background, format).repeat((size[0] * size[1]) as usize);
                gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: renderer.composition_texture(),
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
                    renderer.composition_texture().size(),
                );
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                assert!(renderer.draw_record(&mut encoder, clip, 0));
                let read = gpu.read(&mut encoder, renderer.composition_texture());
                let output = gpu.finish(encoder, &[read], size);
                let mut expected = vec![background; (size[0] * size[1]) as usize];
                let pixels = row["pixels"].as_array().unwrap();
                assert!(!pixels.is_empty(), "native type16 control must edit pixels");
                for pixel in pixels {
                    let offset = (pixel[1].as_u64().unwrap() * u64::from(size[0])
                        + pixel[0].as_u64().unwrap()) as usize;
                    expected[offset] = pixel[2].as_u64().unwrap() as u16;
                }
                for (offset, (pixel, &word)) in output[0]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(&expected)
                    .enumerate()
                {
                    let rgb = if format == wgpu::TextureFormat::Bgra8UnormSrgb {
                        [pixel[2], pixel[1], pixel[0]]
                    } else {
                        [pixel[0], pixel[1], pixel[2]]
                    };
                    // The combat-light shader's existing low-bit expansion
                    // residual is outside the native RGB565 word domain.
                    assert_eq!(
                        RGB565.pack_rgb8(rgb),
                        word,
                        "type16 stage{} radius{} {format:?} pixel{offset}",
                        row["stage"],
                        row["radius"]
                    );
                    assert_eq!(pixel[3], 255);
                }
            }
        }
    }

    fn native_channel(destination: u8, mask: u8, flags: u32, disable_bit: u32) -> u8 {
        let quantized = if disable_bit == 4 {
            destination & 0xfc
        } else {
            destination & 0xf8
        };
        let transformed = if flags & 1 != 0 {
            ((u16::from(quantized) * (256 - u16::from(mask))) >> 8) as u8
        } else if flags & disable_bit != 0 {
            quantized
        } else {
            ((u16::from(quantized) * (256 + u16::from(mask))) >> 8).min(255) as u8
        };
        if disable_bit == 4 {
            transformed & 0xfc
        } else {
            transformed & 0xf8
        }
    }

    fn cpu_draw_native_light(
        coord: crate::sim::projectile::ProjectileCoord,
        surface_index: usize,
        flags: u32,
        camera: [i32; 2],
        zoom: i32,
        size: [usize; 2],
        background: [u8; 3],
    ) -> ([i32; 2], Option<[i32; 4]>, Vec<[u8; 3]>) {
        let anchor = project_combat_light_anchor(coord);
        let top_left = [
            (anchor[0] - camera[0]) * zoom - MASK_WIDTH as i32 / 2,
            (anchor[1] - camera[1]) * zoom - MASK_HEIGHT as i32 / 2,
        ];
        let masks = build_native_mask_atlas();
        let layer_stride = MASK_WIDTH as usize * MASK_HEIGHT as usize;
        let layer = &masks[surface_index * layer_stride..(surface_index + 1) * layer_stride];
        let mut pixels = vec![background; size[0] * size[1]];
        let mut bounds: Option<[i32; 4]> = None;
        for local_y in 0..MASK_HEIGHT as usize {
            for local_x in 0..MASK_WIDTH as usize {
                let mask = layer[local_y * MASK_WIDTH as usize + local_x];
                if mask == 0 {
                    continue;
                }
                let x = top_left[0] + local_x as i32;
                let y = top_left[1] + local_y as i32;
                if x < 0 || y < 0 || x >= size[0] as i32 || y >= size[1] as i32 {
                    continue;
                }
                pixels[y as usize * size[0] + x as usize] = [
                    native_channel(background[0], mask, flags, 2),
                    native_channel(background[1], mask, flags, 4),
                    native_channel(background[2], mask, flags, 8),
                ];
                bounds = Some(match bounds {
                    None => [x, y, x, y],
                    Some([left, top, right, bottom]) => {
                        [left.min(x), top.min(y), right.max(x), bottom.max(y)]
                    }
                });
            }
        }
        (anchor, bounds, pixels)
    }

    #[test]
    fn gsi_04_07_invulnerability_light_projection_truncates_before_full_rectangle_draw() {
        let width = 320;
        let (anchor, bounds, pixels) = cpu_draw_native_light(
            crate::sim::projectile::ProjectileCoord::new(255, 0, 0),
            63,
            1,
            [0, 0],
            1,
            [width, 200],
            [160, 160, 160],
        );

        assert_eq!(anchor, [29, 29]);
        assert_eq!(bounds, Some([0, 0, 156, 92]));
        assert_eq!(pixels[29 * width + 29], [0, 0, 0]);
        assert_eq!(pixels[29 * width + 156], [152, 152, 152]);
        assert_eq!(pixels[29 * width + 157], [160, 160, 160]);
    }

    #[test]
    fn gsi_04_07_invulnerability_light_masks_use_native_midpoint_surfaces() {
        let atlas = build_native_mask_atlas();
        let stride = MASK_WIDTH as usize * MASK_HEIGHT as usize;
        assert_eq!(atlas.len(), stride * 64);

        let layer0 = &atlas[..stride];
        assert_eq!(layer0[64 * 256 + 128], 12);
        assert_eq!(layer0[64 * 256 + 127], 12);
        assert_eq!(layer0[63 * 256 + 128], 0);

        let layer63 = &atlas[63 * stride..64 * stride];
        assert_eq!(layer63[64 * 256 + 128], 255);
        assert_eq!(layer63[64 * 256 + 1], 12);
        assert_eq!(layer63[64 * 256], 0);
        assert_eq!(layer63[0], 0);
    }

    #[test]
    fn gsi_04_07_invulnerability_light_ic_darkens_and_force_shield_only_brightens_blue() {
        assert_eq!(native_channel(160, 64, 1, 2), 120);
        assert_eq!(native_channel(160, 64, 1, 4), 120);
        assert_eq!(native_channel(160, 64, 1, 8), 120);
        assert_eq!(native_channel(160, 64, 6, 2), 160);
        assert_eq!(native_channel(160, 64, 6, 4), 160);
        assert_eq!(native_channel(160, 64, 6, 8), 200);
    }
}
