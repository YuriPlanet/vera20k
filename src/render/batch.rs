//! Sprite batch renderer — draws many textured quads per frame using GPU instancing.
//!
//! One draw call renders hundreds of sprites, each with its own screen position,
//! size, and UV coordinates. Essential for terrain tiles (hundreds per viewport).
//! Bind group 0 = camera uniform (screen size + scroll offset), bind group 1 = texture.
//! Instance buffer provides per-sprite data as vertex attributes (step_mode = Instance).
//!
//! ## Dependency rules
//! - Part of render/ — depends on render/gpu for GpuContext.

use std::collections::HashMap;

use wgpu::util::DeviceExt;

use crate::render::draw_state::DrawState;
use crate::render::gpu::GpuContext;

/// WGSL shader for instanced sprite rendering (loaded from batch_shader.wgsl).
///
/// Vertex shader: generates quad from vertex_index, applies per-instance position/size,
/// and transforms screen-space pixel coordinates to clip space using the camera uniform.
/// Fragment shader: samples the sprite texture with per-instance UV coordinates.
const BATCH_SHADER: &str = include_str!("batch_shader.wgsl");

/// Type-16 SpotlightClass zero-blend shader.
const BUILDING_LIGHT_SHADER: &str = include_str!("building_light.wgsl");

/// `Dst * (mask / 256) + Dst`, preserving destination alpha.
/// This is destination-dependent fixed-function blending, not alpha blending.
pub const SPOTLIGHT_ZERO_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Dst,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

/// WGSL shader with per-pixel Z-depth output via @builtin(frag_depth).
/// Samples a parallel R8 depth atlas to compute per-pixel terrain depth.
const ZDEPTH_SHADER: &str = include_str!("zdepth_shader.wgsl");

/// WGSL per-pixel Z-tested SHP shader: native row Z from the gradient table,
/// optional BUILDNGZ z-shape subtraction, `frag_depth` on the shared axis.
const ZSPRITE_SHADER: &str = include_str!("zsprite_shader.wgsl");

/// WGSL voxel-sprite shader: byte → (palette | house_ramp) → fx pipeline.
/// Atlas is R8Uint (palette indices); palette + per-house RGB ramp are
/// sampled via PaletteSet (bind group 2).
const VOXEL_SPRITE_SHADER: &str = include_str!("sprite_voxel_shader.wgsl");

/// Per-sprite instance data uploaded to the GPU each frame.
///
/// Each instance defines one textured quad: position on screen, pixel size,
/// UV rectangle within the texture, and depth for the depth buffer.
/// The vertex shader uses these along with the camera uniform to produce
/// clip-space positions with correct depth ordering.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpriteInstance {
    /// Top-left corner position in world/screen pixels.
    pub position: [f32; 2],
    /// Width and height of the sprite in pixels.
    pub size: [f32; 2],
    /// Top-left UV coordinate in the texture (0.0 to 1.0).
    pub uv_origin: [f32; 2],
    /// UV width and height (0.0 to 1.0).
    pub uv_size: [f32; 2],
    /// Depth value for the depth buffer (0.0 = near/front, 1.0 = far/back).
    /// Lower screen_y objects get larger depth (drawn behind).
    pub depth: f32,
    /// RGB color tint from map lighting. [1.0, 1.0, 1.0] = no tint (full brightness).
    /// Values < 1.0 darken, > 1.0 brighten (up to 2.0 cap from the lighting formula).
    pub tint: [f32; 3],
    /// Alpha multiplier for translucency. 1.0 = fully opaque, 0.5 = 50% translucent.
    /// Used for chrono warp "being warped" visual (50% during chrono delay).
    pub alpha: f32,
    /// Representation-neutral visual state resolved from the authoritative
    /// entity before either SHP or voxel instance construction.
    pub draw_state: DrawState,
    /// Native Z term of this draw in screen rows (`native_z`): the
    /// CC_Draw_Shape slot-7 value for sprites, the tile/bridge seed offset for
    /// the zdepth pipeline. Integer-valued; consumed only by the Z-tested
    /// pipelines.
    pub z_adjust: f32,
    /// Gradient entry (bits 0-7, `native_z::ZGradient`) plus
    /// `native_z::Z_GRADIENT_ZSHAPE_FLAG` when the fragment shader must
    /// subtract the BUILDNGZ z-shape at `zshape_origin`.
    pub z_gradient: u32,
    /// World-pixel origin of the z-shape canvas (`native_z::zshape_origin`).
    pub zshape_origin: [f32; 2],
    /// Selected native palette conversion; default preserves precomposed RGBA.
    pub palette_light: crate::render::palette_light::PaletteLight,
}

#[cfg(test)]
mod tests {
    use super::SPOTLIGHT_ZERO_BLEND;

    #[test]
    fn spotlight_pipeline_uses_destination_factor_not_alpha() {
        assert_eq!(
            SPOTLIGHT_ZERO_BLEND.color.src_factor,
            wgpu::BlendFactor::Dst
        );
        assert_eq!(
            SPOTLIGHT_ZERO_BLEND.color.dst_factor,
            wgpu::BlendFactor::One
        );
        assert_eq!(
            SPOTLIGHT_ZERO_BLEND.color.operation,
            wgpu::BlendOperation::Add
        );
        assert_eq!(
            SPOTLIGHT_ZERO_BLEND.alpha.src_factor,
            wgpu::BlendFactor::Zero
        );
        assert_eq!(
            SPOTLIGHT_ZERO_BLEND.alpha.dst_factor,
            wgpu::BlendFactor::One
        );
    }
}

/// Number of vertex attributes in SpriteInstance: 7 base + 4 DrawState fields
/// + 3 native-Z fields (z_adjust, z_gradient, zshape_origin / voxel z_rect).
const INSTANCE_ATTRIBUTE_COUNT: usize = 15;

pub(crate) const SPRITE_INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; INSTANCE_ATTRIBUTE_COUNT] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 8,
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 16,
        shader_location: 2,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 24,
        shader_location: 3,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32,
        offset: 32,
        shader_location: 4,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 36,
        shader_location: 5,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32,
        offset: 48,
        shader_location: 6,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Uint32,
        offset: 52,
        shader_location: 7,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Uint32,
        offset: 56,
        shader_location: 8,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 60,
        shader_location: 9,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 76,
        shader_location: 10,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32,
        offset: 92,
        shader_location: 11,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Uint32,
        offset: 96,
        shader_location: 12,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 100,
        shader_location: 13,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Uint32x4,
        offset: std::mem::offset_of!(SpriteInstance, palette_light) as u64,
        shader_location: 14,
    },
];

/// Size of one SpriteInstance in bytes (4 × vec2f = 32 bytes).
const INSTANCE_STRIDE: u64 = std::mem::size_of::<SpriteInstance>() as u64;

/// Camera uniform data sent to the GPU vertex shader.
///
/// Allows the shader to convert screen-space pixel coordinates into
/// normalized clip space, and to apply camera scrolling.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    /// Viewport width and height in pixels.
    pub screen_size: [f32; 2],
    /// Camera scroll position in world pixels (top-left corner of viewport).
    pub camera_pos: [f32; 2],
    /// Zoom level: 1.0 = native scale, >1.0 = zoomed in, <1.0 = zoomed out.
    pub zoom: f32,
    /// Origin of the legacy CPU world/sort scalar. Compatibility Batch
    /// vertices use this to recover its row before mapping to native Z.
    pub world_origin_y: f32,
    /// Extent of the legacy world/sort scalar, in world rows.
    pub world_height: f32,
    /// Padding for 16-byte alignment.
    pub _pad: f32,
    /// Tactical viewport origin in unscaled native row units. Dirty-clip
    /// rebasing cancels the dirty origin (005F4CD9..CED /00437C49..5C).
    pub native_z_origin_y: f32,
    pub _native_z_pad: f32,
}

/// Legacy normalized world/sort scalar: `1 - (row - origin_y) / world_height`.
/// Compatibility Batch producers retain this CPU representation; their depth
/// vertices convert it to the native storage axis. Native TMP/SHP/VXL fragment
/// values do not depend on these map bounds. Non-tactical frames use [`DepthAxis::NONE`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthAxis {
    pub origin_y: f32,
    pub world_height: f32,
}

impl DepthAxis {
    pub const NONE: Self = Self {
        origin_y: 0.0,
        world_height: 1.0,
    };
}

/// Create an `R8Unorm` texture view from single-channel bytes (z-shape and
/// depth planes read with `textureLoad`).
pub fn create_r8_texture_view(
    gpu: &GpuContext,
    label: &str,
    bytes: &[u8],
    width: u32,
    height: u32,
) -> wgpu::TextureView {
    create_r8_texture_view_on_device(&gpu.device, &gpu.queue, label, bytes, width, height)
}

fn create_r8_texture_view_on_device(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    bytes: &[u8],
    width: u32,
    height: u32,
) -> wgpu::TextureView {
    let texture: wgpu::Texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        bytes,
    );
    texture.create_view(&Default::default())
}

fn source_index_texture_on_device(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bytes: &[u8],
    width: u32,
    height: u32,
) -> wgpu::TextureView {
    assert_eq!(bytes.len(), (width * height) as usize);
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("SHP source palette indices"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Uint,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytes,
        )
        .create_view(&Default::default())
}

/// A GPU texture prepared for batch rendering.
///
/// Created via `BatchRenderer::create_texture()`.
pub struct BatchTexture {
    /// Bind group containing texture view + sampler.
    pub bind_group: wgpu::BindGroup,
    /// Raw texture view — exposed for use by the Z-depth pipeline bind group.
    pub view: wgpu::TextureView,
    /// Texture width in pixels.
    pub width: u32,
    /// Texture height in pixels.
    pub height: u32,
}

/// A reusable GPU instance buffer entry. Tracks the wgpu buffer and its current
/// capacity (in number of SpriteInstance elements). Grows on demand, never shrinks.
struct PooledBuffer {
    /// The GPU buffer. Has VERTEX | COPY_DST usage so we can write_buffer() into it.
    buffer: wgpu::Buffer,
    /// Maximum number of SpriteInstance elements the buffer can hold.
    capacity: usize,
}

/// Pool of named GPU instance buffers that persist across frames.
///
/// Instead of creating and destroying GPU buffers every frame (expensive driver
/// round-trips), this pool keeps buffers alive and overwrites their contents
/// with `queue.write_buffer()`. Buffers grow automatically when needed (2x strategy)
/// but never shrink, avoiding repeated reallocations.
///
/// Usage pattern:
/// 1. Call `upload()` for each named buffer (mutably borrows pool).
/// 2. Call `get()` during the render pass to retrieve buffer refs (immutably borrows pool).
pub struct InstanceBufferPool {
    /// Named buffers keyed by a static string (e.g., "terrain", "units").
    buffers: HashMap<&'static str, PooledBuffer>,
    /// Instance counts for each buffer written this frame.
    /// Stored separately so `get()` can return count without needing the data.
    counts: HashMap<&'static str, u32>,
}

/// Minimum buffer capacity in elements. Avoids tiny buffers that immediately
/// need reallocation. 64 instances × 48 bytes = 3 KB — negligible VRAM.
const MIN_POOL_CAPACITY: usize = 64;

impl InstanceBufferPool {
    /// Create an empty pool. Buffers are allocated lazily on first use.
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
            counts: HashMap::new(),
        }
    }

    /// Upload instance data into a named buffer, reusing/growing as needed.
    ///
    /// On first call for a given key, allocates a new GPU buffer. On subsequent
    /// frames, reuses the existing buffer if it fits, or replaces it with a larger
    /// one (2x growth). Data is uploaded via `queue.write_buffer()` — a simple
    /// memcpy, much cheaper than `create_buffer_init()` which allocates new VRAM.
    ///
    /// If `instances` is empty, the count is set to 0 and no GPU upload occurs.
    pub fn upload(&mut self, gpu: &GpuContext, key: &'static str, instances: &[SpriteInstance]) {
        self.upload_on_device(&gpu.device, &gpu.queue, key, instances);
    }

    pub(crate) fn upload_on_device(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: &'static str,
        instances: &[SpriteInstance],
    ) {
        let needed: usize = instances.len();
        if needed == 0 {
            self.counts.insert(key, 0);
            return;
        }

        let entry: &mut PooledBuffer = self.buffers.entry(key).or_insert_with(|| {
            let cap: usize = needed.max(MIN_POOL_CAPACITY);
            PooledBuffer {
                buffer: Self::alloc_buffer(device, key, cap),
                capacity: cap,
            }
        });

        // Grow if the current buffer is too small.
        if needed > entry.capacity {
            let new_cap: usize = (entry.capacity * 2).max(needed);
            entry.buffer = Self::alloc_buffer(device, key, new_cap);
            entry.capacity = new_cap;
        }

        let byte_data: &[u8] = bytemuck::cast_slice(instances);
        queue.write_buffer(&entry.buffer, 0, byte_data);
        self.counts.insert(key, needed as u32);
    }

    /// Get a previously uploaded buffer and its instance count.
    ///
    /// Returns None if the key was never uploaded or had 0 instances.
    /// Safe to call from the render pass — only borrows &self.
    pub fn get(&self, key: &'static str) -> Option<(&wgpu::Buffer, u32)> {
        let count: u32 = *self.counts.get(key)?;
        if count == 0 {
            return None;
        }
        let entry: &PooledBuffer = self.buffers.get(key)?;
        Some((&entry.buffer, count))
    }

    /// Allocate a GPU buffer with VERTEX + COPY_DST usage for the given capacity.
    fn alloc_buffer(device: &wgpu::Device, label: &str, capacity: usize) -> wgpu::Buffer {
        let byte_size: u64 = (capacity as u64) * (std::mem::size_of::<SpriteInstance>() as u64);
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: byte_size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
}

/// Instanced sprite batch renderer.
///
/// Draws many textured quads in a single draw call using GPU instancing.
/// Call `update_camera()` each frame before drawing.
///
/// Pipelines:
/// - `zdepth_pipeline` (terrain): depth write ON — terrain writes Z-data.
/// - `overlay_pipeline` (UI/debug): depth write ON, LessEqual — for passes that
///   intentionally update the shared depth buffer.
pub struct BatchRenderer {
    /// Render pipeline with depth write ON, LessEqual compare.
    /// Used for UI/debug passes that intentionally write depth.
    overlay_pipeline: wgpu::RenderPipeline,
    /// Per-pixel Z-tested SHP pipeline, depth compare Less, no write: the
    /// native read-only leaves (`0x00494b60` / `0x00497fd0`) for units,
    /// infantry, anims and building overlays.
    zsprite_read_pipeline: wgpu::RenderPipeline,
    /// Per-pixel Z-tested SHP pipeline, depth compare Less, write on: the
    /// building-body leaves (`0x004958d0` / `0x004990e0`) with the BUILDNGZ
    /// z-shape bound at group 2.
    zsprite_write_pipeline: wgpu::RenderPipeline,
    /// Bind group layout for the z-shape texture (group 2 of the zsprite
    /// pipelines): one `R8Unorm` texture read with `textureLoad`.
    zshape_bind_group_layout: wgpu::BindGroupLayout,
    /// 1x1 neutral z-shape used when no BUILDNGZ.SHA is loaded.
    default_zshape_bind_group: wgpu::BindGroup,
    /// Render pipeline with per-pixel Z-depth (frag_depth output, LessEqual
    /// compare like `TMP_TileBlitter`'s `base + zdata <= zbuf`).
    /// Used for terrain tiles with TMP Z-data.
    zdepth_pipeline: wgpu::RenderPipeline,
    /// Render pipeline for non-wall overlays (ore, trees): depth compare Always,
    /// depth write OFF. Overlays draw unconditionally over terrain because
    /// tiles without Z-data skip Z-testing entirely.
    overlay_passthrough_pipeline: wgpu::RenderPipeline,
    /// Destination-factor pipeline for SpotlightClass type-16 zero blending.
    spotlight_zero_blend_pipeline: wgpu::RenderPipeline,
    /// Procedurally generated native type-16 mask bank (R8, nearest sampled).
    spotlight_type16_masks: BatchTexture,
    /// Depth-only pipeline: colour target fully masked, depth write ON, Less
    /// compare. Stamps a sprite's opaque silhouette so a later depth-testing
    /// pass can be clipped by it, without disturbing colour compositing order.
    /// Production no longer stamps (sprite bodies write Z themselves); only the
    /// GPU test of `depth_test_pipeline` lays depth down with it.
    #[cfg(test)]
    depth_stamp_pipeline: wgpu::RenderPipeline,
    /// Depth-testing pipeline: colour ON, depth write OFF, Less compare. The
    /// read-only counterpart of `depth_stamp_pipeline`.
    depth_test_pipeline: wgpu::RenderPipeline,
    /// Layout for texture bind groups (group 1).
    texture_bind_group_layout: wgpu::BindGroupLayout,
    default_source_indices: wgpu::TextureView,
    /// Layout for zdepth texture bind groups (group 1): color + sampler + R8 depth.
    zdepth_texture_bind_group_layout: wgpu::BindGroupLayout,
    /// Layout for the unit-atlas R8Uint texture (binding 0). Used by the voxel
    /// sprite pipeline; tiles store palette indices, not RGB.
    pub unit_atlas_bind_group_layout: wgpu::BindGroupLayout,
    /// Layout for the PaletteSet bind group (palette + house_ramp + sampler).
    /// Stored here so the voxel sprite pipeline can be created at BatchRenderer
    /// init time, before the actual PaletteSet exists. PaletteSet creates a
    /// structurally-identical layout for its own bind group at theater load.
    pub voxel_palette_bind_group_layout: wgpu::BindGroupLayout,
    /// Voxel sprite pipeline: byte → (palette | house_ramp) → FX. Reads from
    /// `unit_atlas_bind_group_layout` (group 1) + `voxel_palette_bind_group_layout`
    /// (group 2). Same vertex layout as the other batch pipelines.
    pub voxel_sprite_pipeline: wgpu::RenderPipeline,
    /// Layout for camera uniform bind group (group 0).
    /// Stored so other pipelines (e.g., fog shader) can reuse the same layout.
    camera_bind_group_layout: wgpu::BindGroupLayout,
    /// Camera uniform buffer (group 0) — world camera with zoom.
    // Exact bytes last written by the camera owner; scissor consumers read
    // this record instead of independently reconstructing scroll/zoom/origin.
    uploaded_camera: std::sync::RwLock<CameraUniform>,
    camera_buffer: wgpu::Buffer,
    /// Camera bind group — world camera with zoom.
    camera_bind_group: wgpu::BindGroup,
    /// UI camera uniform buffer — always zoom=1.0 for screen-fixed elements.
    ui_camera_buffer: wgpu::Buffer,
    /// UI camera bind group — always zoom=1.0.
    ui_camera_bind_group: wgpu::BindGroup,
}

impl BatchRenderer {
    /// Create a new BatchRenderer. Compiles shader, creates pipeline and camera uniform.
    pub fn new(gpu: &GpuContext) -> Self {
        Self::new_with_device(&gpu.device, &gpu.queue, gpu.surface_format)
    }

    pub(crate) fn new_with_device(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let shader: wgpu::ShaderModule =
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Batch Shader"),
                source: wgpu::ShaderSource::Wgsl(
                    crate::render::tactical_shader::source(BATCH_SHADER).into(),
                ),
            });

        // Bind group 0: Camera uniform.
        let camera_bind_group_layout: wgpu::BindGroupLayout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Batch Camera BGL"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        // Bind group 1: Texture + sampler.
        let texture_bind_group_layout: wgpu::BindGroupLayout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Batch Texture BGL"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Uint,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });

        let default_source_indices = source_index_texture_on_device(device, queue, &[1], 1, 1);

        // Camera uniform buffer (initialized with default values).
        let camera_uniform: CameraUniform = CameraUniform {
            screen_size: [1024.0, 768.0],
            camera_pos: [0.0, 0.0],
            zoom: 1.0,
            world_origin_y: 0.0,
            world_height: 1.0,
            _pad: 0.0,

            native_z_origin_y: 0.0,
            _native_z_pad: 0.0,
        };
        let camera_buffer: wgpu::Buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Camera Uniform"),
                contents: bytemuck::cast_slice(&[camera_uniform]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        let camera_bind_group: wgpu::BindGroup =
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Camera Bind Group"),
                layout: &camera_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                }],
            });

        // UI camera — identical layout but always zoom=1.0 for screen-fixed elements.
        let ui_camera_buffer: wgpu::Buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("UI Camera Uniform"),
                contents: bytemuck::cast_slice(&[camera_uniform]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let ui_camera_bind_group: wgpu::BindGroup =
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("UI Camera Bind Group"),
                layout: &camera_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: ui_camera_buffer.as_entire_binding(),
                }],
            });

        // Instance buffer vertex layout (matches SpriteInstance memory layout):
        //   position(8) + size(8) + uv_origin(8) + uv_size(8) = 32 → loc 0-3
        //   depth(4) + tint(12) + alpha(4) = 20 → loc 4-6 (offsets 32, 36, 48)
        //   DrawState::remap_row(4) at offset 52 → loc 7 (Uint32)
        //   DrawState::fx_flags(4) at offset 56 → loc 8 (Uint32)
        //   DrawState::fx_params(16) at offset 60 → loc 9 (Float32x4)
        //   DrawState::effect_tint(16) at offset 76 → loc 10 (Float32x4)
        //   z_adjust(4) at offset 92 → loc 11 (Float32)
        //   z_gradient(4) at offset 96 → loc 12 (Uint32)
        //   zshape_origin(8) at offset 100 → loc 13 (Float32x2)
        // PaletteLight(16) at offset 108 -> loc 14 (Uint32x4).
        // Total stride: 124 bytes.
        let instance_attrs = SPRITE_INSTANCE_ATTRIBUTES;

        let pipeline_layout: wgpu::PipelineLayout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Batch Pipeline Layout"),
                bind_group_layouts: &[&camera_bind_group_layout, &texture_bind_group_layout],
                push_constant_ranges: &[],
            });

        let building_light_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("BuildingLight Type-16 Shader"),
            source: wgpu::ShaderSource::Wgsl(BUILDING_LIGHT_SHADER.into()),
        });
        let spotlight_zero_blend_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("BuildingLight Type-16 Zero Blend Pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &building_light_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: INSTANCE_STRIDE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attrs,
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &building_light_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(SPOTLIGHT_ZERO_BLEND),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Always,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        let spotlight_pixels = crate::render::building_light::generate_type16_mask_bank(
            crate::render::building_light::DEFAULT_SPOTLIGHT_RADIUS,
        );
        let spotlight_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("BuildingLight Type-16 Mask Bank"),
                size: wgpu::Extent3d {
                    width: crate::render::building_light::TYPE16_ATLAS_WIDTH as u32,
                    height: crate::render::building_light::TYPE16_MASK_HEIGHT as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &spotlight_pixels,
        );
        let spotlight_view = spotlight_texture.create_view(&Default::default());
        let spotlight_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("BuildingLight Type-16 Nearest Sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let spotlight_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("BuildingLight Type-16 Mask Bind Group"),
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&spotlight_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&spotlight_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&default_source_indices),
                },
            ],
        });
        let spotlight_type16_masks = BatchTexture {
            bind_group: spotlight_bind_group,
            view: spotlight_view,
            width: crate::render::building_light::TYPE16_ATLAS_WIDTH as u32,
            height: crate::render::building_light::TYPE16_MASK_HEIGHT as u32,
        };

        // Overlay pipeline: depth write ON, LessEqual compare.
        // Used for UI/debug passes that intentionally update depth.
        let overlay_pipeline: wgpu::RenderPipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Batch Pipeline (Overlay Write)"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_depth"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: INSTANCE_STRIDE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attrs,
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // Passthrough pipeline for non-wall overlays: depth compare Always, no write.
        // Tiles without embedded Z-data (flag 0x02 at cell header byte 36) skip
        // Z-testing entirely. Ore, gems, and terrain objects have no Z-data, so
        // they paint unconditionally over terrain.
        let overlay_passthrough_pipeline: wgpu::RenderPipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Batch Pipeline (Overlay Passthrough)"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: INSTANCE_STRIDE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attrs,
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Always,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // Building bodies stamp their silhouette here so the post-shroud
        // selection-bracket redraw can be clipped by the art it stands behind.
        // Colour is masked off entirely, so this pass cannot perturb the
        // painter's-order compositing the Ground band depends on — only the
        // depth attachment changes.
        #[cfg(test)]
        let depth_stamp_pipeline: wgpu::RenderPipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Batch Pipeline (Depth Stamp)"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_depth"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: INSTANCE_STRIDE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attrs,
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        // The whole point: contribute depth, never colour.
                        write_mask: wgpu::ColorWrites::empty(),
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // Read-only counterpart: bracket pixels compare against whatever the
        // stamp left and drop the ones that lose, but never write depth
        // themselves — gamemd gates its own line Z-store behind a caller flag
        // that this path leaves clear.
        let depth_test_pipeline: wgpu::RenderPipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Batch Pipeline (Depth Test)"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_depth"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: INSTANCE_STRIDE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attrs,
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // Z-depth bind group layout: color texture + sampler + R8 depth texture.
        let zdepth_texture_bind_group_layout: wgpu::BindGroupLayout = device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("ZDepth Texture BGL"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });

        // Z-depth pipeline: per-pixel depth via frag_depth, Less compare.
        let zdepth_shader: wgpu::ShaderModule =
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("ZDepth Shader"),
                source: wgpu::ShaderSource::Wgsl(
                    crate::render::tactical_shader::source(ZDEPTH_SHADER).into(),
                ),
            });
        let zdepth_pipeline_layout: wgpu::PipelineLayout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("ZDepth Pipeline Layout"),
                bind_group_layouts: &[&camera_bind_group_layout, &zdepth_texture_bind_group_layout],
                push_constant_ranges: &[],
            });
        let zdepth_pipeline: wgpu::RenderPipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("ZDepth Pipeline (Terrain)"),
                layout: Some(&zdepth_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &zdepth_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: INSTANCE_STRIDE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attrs,
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &zdepth_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    // `TMP_TileBlitter @ 0x00547CF0`: `base + zdata <= zbuf`.
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // Z-shape bind group layout (group 2 of the zsprite pipelines).
        let zshape_bind_group_layout: wgpu::BindGroupLayout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("ZShape BGL"),
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
        let default_zshape_view: wgpu::TextureView = create_r8_texture_view_on_device(
            device,
            queue,
            "Default ZShape (neutral)",
            &[crate::render::native_z::ZSHAPE_TEXEL_BIAS as u8],
            1,
            1,
        );
        let default_zshape_bind_group: wgpu::BindGroup =
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Default ZShape BG"),
                layout: &zshape_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&default_zshape_view),
                }],
            });

        // Per-pixel Z-tested SHP pipelines: native row Z + optional z-shape.
        let zsprite_shader: wgpu::ShaderModule =
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("ZSprite Shader"),
                source: wgpu::ShaderSource::Wgsl(
                    crate::render::tactical_shader::source(ZSPRITE_SHADER).into(),
                ),
            });
        let zsprite_pipeline_layout: wgpu::PipelineLayout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("ZSprite Pipeline Layout"),
                bind_group_layouts: &[
                    &camera_bind_group_layout,
                    &texture_bind_group_layout,
                    &zshape_bind_group_layout,
                ],
                push_constant_ranges: &[],
            });
        let make_zsprite_pipeline = |label: &str, depth_write_enabled: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&zsprite_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &zsprite_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: INSTANCE_STRIDE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attrs,
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &zsprite_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled,
                    // Sprite leaves test `z < zbuf` (strict).
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let zsprite_read_pipeline: wgpu::RenderPipeline =
            make_zsprite_pipeline("ZSprite Pipeline (read-only)", false);
        let zsprite_write_pipeline: wgpu::RenderPipeline =
            make_zsprite_pipeline("ZSprite Pipeline (read + write)", true);

        // Bind group layout for the unit-atlas R8Uint texture (voxel sprite path).
        // Single texture entry, no sampler — sampled via textureLoad with integer coords.
        let unit_atlas_bind_group_layout: wgpu::BindGroupLayout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("unit_atlas_bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }],
            });

        // Bind group layout for the PaletteSet (palette texture + house_ramp
        // texture + non-filtering sampler). Must structurally match
        // PaletteSet::new()'s layout (wgpu compares layouts structurally, not
        // by reference, so two distinct BindGroupLayouts with identical
        // entries are interchangeable).
        let voxel_palette_bind_group_layout: wgpu::BindGroupLayout = device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("voxel_palette_bgl_for_pipeline"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                        count: None,
                    },
                ],
            });

        // Voxel sprite pipeline: 3 bind groups (camera, atlas R8Uint, PaletteSet).
        // Same vertex layout as the existing batch pipelines.
        // Depth: write OFF, compare Less — the native VXL cache blit reaches
        // the read-only leaf `0x00497fd0` (`z < zbuf`, no store), so voxel
        // bodies test against terrain and building Z but never write.
        let voxel_shader: wgpu::ShaderModule =
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Voxel Sprite Shader"),
                source: wgpu::ShaderSource::Wgsl(
                    crate::render::tactical_shader::source(VOXEL_SPRITE_SHADER).into(),
                ),
            });
        let voxel_sprite_pipeline_layout: wgpu::PipelineLayout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Voxel Sprite Pipeline Layout"),
                bind_group_layouts: &[
                    &camera_bind_group_layout,
                    &unit_atlas_bind_group_layout,
                    &voxel_palette_bind_group_layout,
                ],
                push_constant_ranges: &[],
            });
        let voxel_sprite_pipeline: wgpu::RenderPipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Voxel Sprite Pipeline"),
                layout: Some(&voxel_sprite_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &voxel_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: INSTANCE_STRIDE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attrs,
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &voxel_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        Self {
            overlay_pipeline,
            zsprite_read_pipeline,
            zsprite_write_pipeline,
            zshape_bind_group_layout,
            default_zshape_bind_group,
            zdepth_pipeline,
            overlay_passthrough_pipeline,
            spotlight_zero_blend_pipeline,
            spotlight_type16_masks,
            #[cfg(test)]
            depth_stamp_pipeline,
            depth_test_pipeline,
            texture_bind_group_layout,
            default_source_indices,
            zdepth_texture_bind_group_layout,
            unit_atlas_bind_group_layout,
            voxel_palette_bind_group_layout,
            voxel_sprite_pipeline,
            camera_bind_group_layout,
            camera_buffer,
            uploaded_camera: std::sync::RwLock::new(camera_uniform),
            camera_bind_group,
            ui_camera_buffer,
            ui_camera_bind_group,
        }
    }

    /// Create a single-channel R8Uint atlas texture from byte data.
    ///
    /// Used for voxel sprite atlases where each byte is a palette index
    /// (post-VPL, pre-house-remap). Sampled in shader via `textureLoad` (no
    /// filtering, integer coords).
    pub fn create_unit_atlas_texture(
        &self,
        gpu: &GpuContext,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> BatchTexture {
        self.create_unit_atlas_texture_on_device(&gpu.device, &gpu.queue, width, height, pixels)
    }

    pub(crate) fn create_unit_atlas_texture_on_device(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> BatchTexture {
        self.create_unit_atlas_texture_parts(device, queue, width, height, pixels)
            .1
    }

    /// An empty R8Uint unit-atlas page that its owner rewrites in place
    /// (`queue.write_texture` on the returned texture) instead of recreating
    /// it and its bind group.
    pub fn create_updatable_unit_atlas_texture(
        &self,
        gpu: &GpuContext,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, BatchTexture) {
        let pixels = vec![0; (width * height) as usize];
        self.create_unit_atlas_texture_parts(&gpu.device, &gpu.queue, width, height, &pixels)
    }

    fn create_unit_atlas_texture_parts(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> (wgpu::Texture, BatchTexture) {
        debug_assert_eq!(
            pixels.len(),
            (width * height) as usize,
            "pixel buffer size must equal width * height"
        );

        let texture: wgpu::Texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("unit_atlas_r8uint"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let view: wgpu::TextureView = texture.create_view(&Default::default());
        let bind_group: wgpu::BindGroup = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("unit_atlas_bg"),
            layout: &self.unit_atlas_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });

        (
            texture,
            BatchTexture {
                bind_group,
                view,
                width,
                height,
            },
        )
    }

    /// Upload RGBA pixel data to the GPU as a batch-renderable texture.
    ///
    /// Uses nearest-neighbor sampling (pixel art). The returned BatchTexture
    /// can be shared across multiple draw calls.
    pub fn create_texture(
        &self,
        gpu: &GpuContext,
        rgba_data: &[u8],
        width: u32,
        height: u32,
    ) -> BatchTexture {
        self.create_texture_with_indices(gpu, rgba_data, width, height, None)
    }

    pub fn create_texture_with_indices(
        &self,
        gpu: &GpuContext,
        rgba_data: &[u8],
        width: u32,
        height: u32,
        indices: Option<&[u8]>,
    ) -> BatchTexture {
        self.create_texture_on_device(&gpu.device, &gpu.queue, rgba_data, width, height, indices)
    }

    pub(crate) fn create_texture_on_device(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba_data: &[u8],
        width: u32,
        height: u32,
        indices: Option<&[u8]>,
    ) -> BatchTexture {
        let source_indices = indices
            .map(|bytes| source_index_texture_on_device(device, queue, bytes, width, height));
        let source_indices = source_indices
            .as_ref()
            .unwrap_or(&self.default_source_indices);
        let texture: wgpu::Texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("Batch Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            rgba_data,
        );

        let view: wgpu::TextureView = texture.create_view(&Default::default());
        let sampler: wgpu::Sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Batch Sampler (Nearest)"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group: wgpu::BindGroup = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Batch Texture BG"),
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(source_indices),
                },
            ],
        });

        BatchTexture {
            bind_group,
            view,
            width,
            height,
        }
    }

    /// Update the camera uniform with current viewport size and scroll position.
    ///
    /// Call once per frame before drawing. screen_width/height are in pixels.
    /// camera_x/y define the top-left corner of the visible area in world coordinates.
    pub fn update_camera(
        &self,
        gpu: &GpuContext,
        screen_width: f32,
        screen_height: f32,
        camera_x: f32,
        camera_y: f32,
        zoom: f32,
        depth_axis: DepthAxis,
    ) {
        self.update_camera_on_queue(
            &gpu.queue,
            screen_width,
            screen_height,
            camera_x,
            camera_y,
            zoom,
            depth_axis,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_camera_on_queue(
        &self,
        queue: &wgpu::Queue,
        screen_width: f32,
        screen_height: f32,
        camera_x: f32,
        camera_y: f32,
        zoom: f32,
        depth_axis: DepthAxis,
    ) {
        // Round camera to integer pixels — sub-pixel camera offsets cause
        // visible seams between adjacent terrain tiles.
        let cam = [camera_x.round(), camera_y.round()];
        // `RA2_DEBUG_DEPTH_VIEW=1`: the Z-tested shaders paint their fragment
        // depth as grey (wrapping every 128 world rows) instead of colour.
        static DEBUG_DEPTH_VIEW: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
        let debug_depth_view = *DEBUG_DEPTH_VIEW.get_or_init(|| {
            if std::env::var_os("RA2_DEBUG_DEPTH_VIEW").is_some() {
                1.0
            } else {
                0.0
            }
        });
        let uniform: CameraUniform = CameraUniform {
            screen_size: [screen_width, screen_height],
            camera_pos: cam,
            zoom,
            world_origin_y: depth_axis.origin_y,
            world_height: depth_axis.world_height.max(1.0),
            _pad: debug_depth_view,

            native_z_origin_y: 0.0,
            _native_z_pad: 0.0,
        };
        self.write_camera(queue, uniform);
    }

    /// Preserve the full tactical viewport origin separately from camera
    /// scrolling. The argument uses unscaled native rows, not target pixels.
    pub(crate) fn update_native_z_origin(&self, queue: &wgpu::Queue, origin_y: f32) {
        let mut uploaded = self.uploaded_camera.write().expect("camera owner lock");
        uploaded.native_z_origin_y = origin_y;
        let offset = std::mem::offset_of!(CameraUniform, native_z_origin_y) as u64;
        queue.write_buffer(&self.camera_buffer, offset, bytemuck::bytes_of(&origin_y));
        queue.write_buffer(
            &self.ui_camera_buffer,
            offset,
            bytemuck::bytes_of(&origin_y),
        );
    }

    pub(crate) fn camera_uniform(&self) -> CameraUniform {
        *self.uploaded_camera.read().expect("camera owner lock")
    }

    pub(crate) fn write_camera(&self, queue: &wgpu::Queue, uniform: CameraUniform) {
        let mut uploaded = self.uploaded_camera.write().expect("camera owner lock");
        *uploaded = uniform;
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&uniform));
        let ui_uniform = CameraUniform {
            zoom: 1.0,
            _pad: 0.0,
            ..uniform
        };
        queue.write_buffer(&self.ui_camera_buffer, 0, bytemuck::bytes_of(&ui_uniform));
    }

    /// Create a standalone instance buffer (not stored in the renderer).
    ///
    /// Use this when drawing multiple batches per render pass — each batch
    /// gets its own buffer that stays alive until the render pass ends.
    /// Returns None if instances is empty.
    pub fn create_instance_buffer(
        &self,
        gpu: &GpuContext,
        instances: &[SpriteInstance],
    ) -> Option<(wgpu::Buffer, u32)> {
        if instances.is_empty() {
            return None;
        }
        let buffer: wgpu::Buffer =
            gpu.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("External Batch Instances"),
                    contents: bytemuck::cast_slice(instances),
                    usage: wgpu::BufferUsages::VERTEX,
                });
        Some((buffer, instances.len() as u32))
    }

    /// Draw a sub-range of voxel sprite instances using the voxel sprite pipeline.
    /// Used by the multi-way Y-sort merge passes that interleave voxel and SHP
    /// draw calls, from a start index.
    pub fn draw_voxel_sprites_range<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        atlas: &'a BatchTexture,
        palette_bind_group: &'a wgpu::BindGroup,
        buffer: &'a wgpu::Buffer,
        start: u32,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        render_pass.set_pipeline(&self.voxel_sprite_pipeline);
        render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
        render_pass.set_bind_group(1, &atlas.bind_group, &[]);
        render_pass.set_bind_group(2, palette_bind_group, &[]);
        render_pass.set_vertex_buffer(0, buffer.slice(..));
        render_pass.draw(0..6, start..start + count);
    }

    /// Access the camera bind group for use by external pipelines (e.g., fog shader).
    pub fn camera_bind_group(&self) -> &wgpu::BindGroup {
        &self.camera_bind_group
    }

    /// Share the production RGBA/source-index binding layout.
    pub(crate) fn texture_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.texture_bind_group_layout
    }

    /// Access the camera bind group layout so external pipelines can share it.
    pub fn camera_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.camera_bind_group_layout
    }

    /// Access the UI camera bind group (zoom=1.0) for screen-fixed elements.
    pub fn ui_camera_bind_group(&self) -> &wgpu::BindGroup {
        &self.ui_camera_bind_group
    }

    /// Access the overlay (no-depth) pipeline for manual draw calls.
    pub fn overlay_pipeline(&self) -> &wgpu::RenderPipeline {
        &self.overlay_pipeline
    }

    /// Create a reusable texture that supports `queue.write_texture()` updates.
    ///
    /// Returns both the raw `wgpu::Texture` (needed for write_texture) and the
    /// `BatchTexture` (needed for draw calls). The texture is created with
    /// `TEXTURE_BINDING | COPY_DST` usage so it can be updated each frame
    /// without recreating the bind group.
    pub fn create_updatable_texture(
        &self,
        gpu: &GpuContext,
        rgba_data: &[u8],
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, BatchTexture) {
        let texture: wgpu::Texture = gpu.device.create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("Updatable Batch Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            rgba_data,
        );

        let view: wgpu::TextureView = texture.create_view(&Default::default());
        let sampler: wgpu::Sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Updatable Batch Sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group: wgpu::BindGroup =
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Updatable Batch Texture BG"),
                layout: &self.texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&self.default_source_indices),
                    },
                ],
            });

        let batch_tex = BatchTexture {
            bind_group,
            view,
            width,
            height,
        };
        (texture, batch_tex)
    }

    /// Draw instances using the overlay pipeline (LessEqual, depth write ON).
    ///
    /// Used for UI/debug passes that intentionally write depth.
    pub fn draw_with_buffer_no_depth<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        texture: &'a BatchTexture,
        buffer: &'a wgpu::Buffer,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        render_pass.set_pipeline(&self.overlay_pipeline);
        render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
        render_pass.set_bind_group(1, &texture.bind_group, &[]);
        render_pass.set_vertex_buffer(0, buffer.slice(..));
        render_pass.draw(0..6, 0..count);
    }

    /// Create a bind group for the Z-depth pipeline (color + sampler + R8 depth).
    ///
    /// The color texture view and depth texture view must have identical UV layout
    /// (same atlas dimensions and tile placements) so the shader can sample both
    /// at the same UV coordinates.
    pub fn create_zdepth_bind_group(
        &self,
        gpu: &GpuContext,
        color_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        self.create_zdepth_bind_group_on_device(&gpu.device, color_view, depth_view)
    }

    pub(crate) fn create_zdepth_bind_group_on_device(
        &self,
        device: &wgpu::Device,
        color_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let sampler: wgpu::Sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ZDepth Sampler (Nearest)"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ZDepth Bind Group"),
            layout: &self.zdepth_texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(depth_view),
                },
            ],
        })
    }

    /// Draw instances with the Z-depth pipeline (per-pixel frag_depth, Less compare).
    ///
    /// Used for terrain tiles with TMP Z-data. The bind_group must be created via
    /// `create_zdepth_bind_group()` with matching color + depth atlas textures.
    pub fn draw_with_buffer_zdepth<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        bind_group: &'a wgpu::BindGroup,
        buffer: &'a wgpu::Buffer,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        render_pass.set_pipeline(&self.zdepth_pipeline);
        render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
        render_pass.set_bind_group(1, bind_group, &[]);
        render_pass.set_vertex_buffer(0, buffer.slice(..));
        render_pass.draw(0..6, 0..count);
    }

    /// Draw sprites/overlays with depth test bypassed (Always compare).
    /// Sprites never interact with the Z-buffer — painted over terrain unconditionally.
    pub fn draw_with_buffer_passthrough<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        texture: &'a BatchTexture,
        buffer: &'a wgpu::Buffer,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        render_pass.set_pipeline(&self.overlay_passthrough_pipeline);
        render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
        render_pass.set_bind_group(1, &texture.bind_group, &[]);
        render_pass.set_vertex_buffer(0, buffer.slice(..));
        render_pass.draw(0..6, 0..count);
    }

    /// Draw authoritative child-light mask quads through the native
    /// destination-dependent zero-blend equation. The current RGBA/BGRA target
    /// cannot reproduce native RGB565 extraction/repack rounding; that target
    /// format boundary remains explicit rather than being approximated here.
    pub fn draw_spotlight_type16<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        buffer: &'a wgpu::Buffer,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        render_pass.set_pipeline(&self.spotlight_zero_blend_pipeline);
        render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
        render_pass.set_bind_group(1, &self.spotlight_type16_masks.bind_group, &[]);
        render_pass.set_vertex_buffer(0, buffer.slice(..));
        render_pass.draw(0..6, 0..count);
    }

    /// Draw with depth bypassed AND the UI camera (zoom=1.0, screen-fixed).
    /// Same as `draw_with_buffer_passthrough` but binds the UI camera so the
    /// instances stay at fixed screen positions regardless of game zoom — used by
    /// the in-game Options overlay, whose instance positions are pre-offset by the
    /// camera scroll (so the UI camera nets the intended screen pixels) and which
    /// must NOT re-upload the world camera mid-frame.
    pub fn draw_with_buffer_ui_passthrough<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        texture: &'a BatchTexture,
        buffer: &'a wgpu::Buffer,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        render_pass.set_pipeline(&self.overlay_passthrough_pipeline);
        render_pass.set_bind_group(0, &self.ui_camera_bind_group, &[]);
        render_pass.set_bind_group(1, &texture.bind_group, &[]);
        render_pass.set_vertex_buffer(0, buffer.slice(..));
        render_pass.draw(0..6, 0..count);
    }

    /// Stamp a sprite's opaque silhouette into the depth buffer, writing no
    /// colour at all.
    ///
    /// This is how a building body gets to occlude the selection-bracket
    /// redraw. gamemd's building blit tests and writes the Z-buffer as it
    /// paints, so the shape's own transparent pixels leave Z untouched; here
    /// the fragment shader's existing alpha discard does that job and the
    /// colour target is masked off, so the pass contributes depth and nothing
    /// else. Compare is `Less` for the same reason gamemd's blitter tests
    /// before it stores — a body behind nearer terrain must not stamp through
    /// it.
    #[cfg(test)]
    pub fn draw_with_buffer_depth_stamp<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        texture: &'a BatchTexture,
        buffer: &'a wgpu::Buffer,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        render_pass.set_pipeline(&self.depth_stamp_pipeline);
        render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
        render_pass.set_bind_group(1, &texture.bind_group, &[]);
        render_pass.set_vertex_buffer(0, buffer.slice(..));
        render_pass.draw(0..6, 0..count);
    }

    /// Draw sprites that test the depth buffer but never write it.
    ///
    /// gamemd's line rasteriser does exactly this: it compares every pixel
    /// against the Z-buffer and skips the ones that lose, while the store back
    /// into Z is gated behind a separate caller flag that the bracket path
    /// leaves clear. Used for the post-shroud bracket redraw.
    pub fn draw_with_buffer_depth_test<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        texture: &'a BatchTexture,
        buffer: &'a wgpu::Buffer,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        render_pass.set_pipeline(&self.depth_test_pipeline);
        render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
        render_pass.set_bind_group(1, &texture.bind_group, &[]);
        render_pass.set_vertex_buffer(0, buffer.slice(..));
        render_pass.draw(0..6, 0..count);
    }

    /// Bind group for a loaded z-shape texture view (BUILDNGZ.SHA, biased R8).
    pub fn create_zshape_bind_group(
        &self,
        gpu: &GpuContext,
        view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ZShape BG"),
            layout: &self.zshape_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            }],
        })
    }

    /// Neutral 1x1 z-shape for draws that carry no BUILDNGZ.
    pub fn default_zshape_bind_group(&self) -> &wgpu::BindGroup {
        &self.default_zshape_bind_group
    }

    /// Draw a sub-range of SHP sprites with the native per-pixel Z test.
    ///
    /// `write` selects the building-body leaf behaviour (store the nearer Z)
    /// over the read-only one; both compare `Less`. `zshape` is the z-shape
    /// bind group the instances' `zshape_origin` refers to.
    pub fn draw_zsprite_range<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        texture: &'a BatchTexture,
        zshape: &'a wgpu::BindGroup,
        buffer: &'a wgpu::Buffer,
        start: u32,
        count: u32,
        write: bool,
    ) {
        if count == 0 {
            return;
        }
        render_pass.set_pipeline(if write {
            &self.zsprite_write_pipeline
        } else {
            &self.zsprite_read_pipeline
        });
        render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
        render_pass.set_bind_group(1, &texture.bind_group, &[]);
        render_pass.set_bind_group(2, zshape, &[]);
        render_pass.set_vertex_buffer(0, buffer.slice(..));
        render_pass.draw(0..6, start..start + count);
    }

    /// Draw a sub-range of sprites with depth test bypassed (Always compare).
    /// Used for the multi-way merge of Y-sorted VXL + SHP draw groups.
    pub fn draw_passthrough_range<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        texture: &'a BatchTexture,
        buffer: &'a wgpu::Buffer,
        start: u32,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        render_pass.set_pipeline(&self.overlay_passthrough_pipeline);
        render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
        render_pass.set_bind_group(1, &texture.bind_group, &[]);
        render_pass.set_vertex_buffer(0, buffer.slice(..));
        render_pass.draw(0..6, start..start + count);
    }
}
