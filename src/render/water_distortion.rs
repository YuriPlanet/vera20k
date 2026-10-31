//! Frame-sized water distortion field for the post-process chain.
//!
//! The engine's water mask ([`crate::render::water_mask::WaterMask`]) marks the
//! visible water diamonds; this pass turns that mask into a smooth RG16F
//! sampling-offset field, the equivalent of the `DISTORT` semantic texture the
//! SAGE presentation hook exposes. The post-process pass samples the field
//! bilinearly and offsets the composed frame by
//! `(rg - 0.5) * 2 * amplitude` inside water only.
//!
//! Depends on: `render/gpu.rs` (through the caller) and wgpu only.

const SHADER_SRC: &str = include_str!("water_distortion.wgsl");

/// Uniform payload for [`WaterDistortion`]; one vec4 keeps std140 alignment
/// trivial.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DistortionParams {
    /// Noise cell size in frame pixels.
    scale: f32,
    /// Scroll speed, in noise cells per second.
    speed: f32,
    /// Seconds since presentation started.
    time: f32,
    _pad: f32,
}

impl DistortionParams {
    pub(crate) fn new(scale: f32, speed: f32, time: f32) -> Self {
        Self {
            scale,
            speed,
            time,
            _pad: 0.0,
        }
    }
}

pub(crate) struct WaterDistortion {
    size: (u32, u32),
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    params_buffer: wgpu::Buffer,
}

impl WaterDistortion {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue, size: (u32, u32)) -> Self {
        let size = (size.0.max(1), size.1.max(1));
        let texture = create_field_texture(device, size);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Water distortion params"),
            size: std::mem::size_of::<DistortionParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &params_buffer,
            0,
            bytemuck::bytes_of(&DistortionParams::new(1.0, 0.0, 0.0)),
        );

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Water distortion"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
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
                ],
            });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Water distortion"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Water distortion"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Water distortion"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rg16Float,
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

        Self {
            size,
            texture,
            view,
            pipeline,
            bind_group_layout,
            params_buffer,
        }
    }

    pub(crate) fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Fill the field from the water mask: neutral outside water, the scrolling
    /// two-octave offset inside it.
    pub(crate) fn generate(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        mask: &wgpu::TextureView,
        params: DistortionParams,
    ) {
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
        let bind_group = self.device_bind_group(device, mask);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Water distortion"),
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn device_bind_group(
        &self,
        device: &wgpu::Device,
        mask: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Water distortion"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(mask),
                },
            ],
        })
    }

    /// Follow a new game-frame size.
    pub(crate) fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        let size = (size.0.max(1), size.1.max(1));
        if size == self.size {
            return;
        }
        let texture = create_field_texture(device, size);
        self.view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.texture = texture;
        self.size = size;
    }

    #[cfg(test)]
    pub(crate) fn test_texture(&self) -> &wgpu::Texture {
        &self.texture
    }
}

fn create_field_texture(device: &wgpu::Device, size: (u32, u32)) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Water distortion"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rg16Float,
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
    fn water_distortion_shader_compiles() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(SHADER_SRC)
            .expect("the distortion shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("the distortion shader validates");
    }
}
