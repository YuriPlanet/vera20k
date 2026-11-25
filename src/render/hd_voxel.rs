//! VERA's HD art voxels: the unit atlas's twice-resolution pictures drawn
//! over the wheel-zoomed world.
//!
//! VERA-only presentation extension (HD art (`[VERA] HdArt=`, `render::hd_switch`), off by default), the
//! voxel side of [`super::hd_art`]. With it on, every voxel picture the unit
//! atlas rasterizes also gets a picture at twice the resolution from the
//! same native stores ([`super::vxl_raster::VxlHd`]), on a twice-size page
//! ([`super::unit_atlas::UnitAtlas::hd_page`]). When the wheel zoom
//! magnifies the world, this pass replaces the pixels of the zoomed copy that
//! the native world still shows as a picture's own colour with the HD
//! picture's (`hd_voxel.wgsl`). The native world and its draws are
//! unchanged; nothing reaches `sim/`, a save, a replay or the network.

use bytemuck::{Pod, Zeroable};

/// One HD voxel draw: a unit atlas piece placed in the native composition.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub(crate) struct HdVoxelDraw {
    pub origin: [f32; 2],
    pub size: [f32; 2],
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub remap_row: u32,
    pub source_palette: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    frame: [f32; 4],
}

/// The draws of one atlas page.
pub(crate) struct HdVoxelPage<'a> {
    pub native: &'a wgpu::TextureView,
    pub hd: &'a wgpu::TextureView,
    pub draws: Vec<HdVoxelDraw>,
}

pub(crate) struct HdVoxelPass {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    pipeline: Option<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    uniform: wgpu::Buffer,
    instances: wgpu::Buffer,
    instance_capacity: usize,
}

fn shader_source() -> String {
    format!(
        "{}\n{}",
        super::webgl::srgb_wgsl(),
        include_str!("hd_voxel.wgsl")
    )
}

impl HdVoxelPass {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
            ty,
            count: None,
        };
        let tex = |sample_type| wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let unfiltered = wgpu::TextureSampleType::Float { filterable: false };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("HD voxels"),
            entries: &[
                entry(0, tex(unfiltered)),
                entry(1, tex(wgpu::TextureSampleType::Uint)),
                entry(2, tex(wgpu::TextureSampleType::Uint)),
                entry(3, tex(unfiltered)),
                entry(4, tex(unfiltered)),
                entry(
                    5,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("HD voxels"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("HD voxels"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("HD voxels params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let instance_capacity = 256;
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("HD voxel draws"),
            size: (instance_capacity * std::mem::size_of::<HdVoxelDraw>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            layout,
            pipeline_layout,
            shader,
            pipeline: None,
            uniform,
            instances,
            instance_capacity,
        }
    }

    /// Draw `pages` into `target` (the zoomed frame, `scale` frame pixels per
    /// native pixel) over `scissor`, reading the native `composition` and
    /// the voxel palette and house ramp.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        composition: &wgpu::Texture,
        palette: &wgpu::TextureView,
        house_ramp: &wgpu::TextureView,
        scale: f32,
        scissor: [u32; 4],
        pages: &[HdVoxelPage<'_>],
        debug: bool,
    ) {
        let total: usize = pages.iter().map(|page| page.draws.len()).sum();
        if total == 0 || scissor[2] == 0 || scissor[3] == 0 {
            return;
        }
        let size = target.size();
        let right = (scissor[0] + scissor[2]).min(size.width);
        let bottom = (scissor[1] + scissor[3]).min(size.height);
        if right <= scissor[0] || bottom <= scissor[1] {
            return;
        }
        let format = target.format();
        if self
            .pipeline
            .as_ref()
            .is_none_or(|(cached, _)| *cached != format)
        {
            self.pipeline = Some((
                format,
                create_pipeline(device, &self.pipeline_layout, &self.shader, format),
            ));
        }
        if total > self.instance_capacity {
            self.instance_capacity = total.next_power_of_two();
            self.instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("HD voxel draws"),
                size: (self.instance_capacity * std::mem::size_of::<HdVoxelDraw>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let all: Vec<HdVoxelDraw> = pages
            .iter()
            .flat_map(|page| page.draws.iter().copied())
            .collect();
        queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&all));
        let params = Params {
            frame: [
                size.width as f32,
                size.height as f32,
                scale,
                if debug { 1.0 } else { 0.0 },
            ],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&params));
        let composition = composition.create_view(&Default::default());
        let groups: Vec<wgpu::BindGroup> = pages
            .iter()
            .map(|page| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("HD voxels"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&composition),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(page.native),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(page.hd),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(palette),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(house_ramp),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: self.uniform.as_entire_binding(),
                        },
                    ],
                })
            })
            .collect();
        let target_view = target.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("HD voxels"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
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
        pass.set_scissor_rect(
            scissor[0],
            scissor[1],
            right - scissor[0],
            bottom - scissor[1],
        );
        pass.set_pipeline(&self.pipeline.as_ref().expect("pipeline").1);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        let mut first = 0u32;
        for (page, group) in pages.iter().zip(&groups) {
            let count = page.draws.len() as u32;
            if count > 0 {
                pass.set_bind_group(0, group, &[]);
                pass.draw(0..4, first..first + count);
            }
            first += count;
        }
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let attributes = wgpu::vertex_attr_array![
        0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x2, 4 => Uint32, 5 => Uint32
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("HD voxels"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<HdVoxelDraw>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &attributes,
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;

    #[test]
    fn hd_voxel_shader_parses_and_validates() {
        use wgpu::naga;

        let source = shader_source();
        let module = naga::front::wgsl::parse_str(&source).expect("the HD voxel shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("the HD voxel shader validates");
    }
}
