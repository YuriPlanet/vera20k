//! Frame-sized water mask for the post-process chain.
//!
//! `TerrainInstances::water` carries the visible water cells' iso diamonds in
//! frame pixels (the same rounded camera subtraction the sprite shader uses);
//! this pass rasterizes them into an `R8Unorm` target that the post-process
//! shader samples with `textureLoad`. Only an engine-side cell predicate
//! (`LandType::Water`) can produce a mask entry, so the displacement never
//! guesses water from colour or depth like a generic ReShade shader would.
//!
//! Depends on: `render/gpu.rs` (through the caller) and wgpu only.

const SHADER_SRC: &str = include_str!("water_mask.wgsl");

/// Six vertices per cell diamond (two triangles).
const VERTS_PER_QUAD: usize = 6;
/// Enough for the visible diamond count of any tactical viewport; larger
/// counts are truncated rather than reallocating per frame.
const MAX_QUADS: usize = 4096;

pub(crate) struct WaterMask {
    size: (u32, u32),
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    size_buffer: wgpu::Buffer,
    vertices: wgpu::Buffer,
}

impl WaterMask {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue, size: (u32, u32)) -> Self {
        let size = (size.0.max(1), size.1.max(1));
        let texture = create_mask_texture(device, size);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let size_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Water mask size"),
            size: 8,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &size_buffer,
            0,
            bytemuck::cast_slice(&[size.0 as f32, size.1 as f32]),
        );

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Water mask size layout"),
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
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Water mask size"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: size_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Water mask"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Water mask"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Water mask"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    }],
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Water mask vertices"),
            size: (MAX_QUADS * VERTS_PER_QUAD * 8) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            size,
            texture,
            view,
            pipeline,
            bind_group,
            size_buffer,
            vertices,
        }
    }

    pub(crate) fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Clear the mask to 0 and rasterize the visible water diamonds.
    pub(crate) fn draw(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        water: &[[f32; 4]],
    ) {
        let quads = water.len().min(MAX_QUADS);
        let mut vertices = Vec::with_capacity(quads * VERTS_PER_QUAD);
        for [x, y, width, height] in water.iter().take(quads) {
            let (x, y, w, h) = (*x, *y, *width, *height);
            let top = [x + w * 0.5, y];
            let right = [x + w, y + h * 0.5];
            let bottom = [x + w * 0.5, y + h];
            let left = [x, y + h * 0.5];
            vertices.extend_from_slice(&[top, right, bottom, top, bottom, left]);
        }
        if !vertices.is_empty() {
            queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&vertices));
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Water mask"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if quads > 0 {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.draw(0..(quads * VERTS_PER_QUAD) as u32, 0..1);
        }
    }

    /// Follow a new game-frame size.
    pub(crate) fn resize(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
    ) {
        let size = (size.0.max(1), size.1.max(1));
        if size == self.size {
            return;
        }
        let texture = create_mask_texture(device, size);
        self.view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.texture = texture;
        queue.write_buffer(
            &self.size_buffer,
            0,
            bytemuck::cast_slice(&[size.0 as f32, size.1 as f32]),
        );
        self.size = size;
    }

    #[cfg(test)]
    pub(crate) fn test_texture(&self) -> &wgpu::Texture {
        &self.texture
    }
}

fn create_mask_texture(device: &wgpu::Device, size: (u32, u32)) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Water mask"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

#[cfg(test)]
mod tests {
    use super::SHADER_SRC;

    #[test]
    fn water_mask_shader_compiles() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(SHADER_SRC).expect("the mask shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("the mask shader validates");
    }
}
