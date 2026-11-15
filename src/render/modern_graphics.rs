//! VERA's Modern picture: a glow, sharpen and grade pass over the finished
//! match frame.
//!
//! VERA-only presentation extension (`[VERA] LookGlow=`, `LookSharpen=`,
//! `LookContrast=`, `LookSaturation=`, levels 0..=10, all off by default): gamemd has no equivalent, nothing here reaches `sim/`, the
//! network, a save or a replay. Classic skips the pass entirely, leaving the
//! original picture byte-identical. The pass runs on the whole frame before
//! the colour-vision filter, so the sidebar takes the look too.
//!
//! Built like `color_vision`: a snapshot copy of the frame, then one
//! full-frame draw back into it (`modern_graphics.wgsl`).

use std::sync::{Mutex, MutexGuard, PoisonError};

use bytemuck::{Pod, Zeroable};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The shader's parameters; see `modern_graphics.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, PartialEq, Debug)]
pub(crate) struct Look {
    /// Glow strength, glow threshold (linear luminance), glow radius in
    /// pixels, sharpen amount.
    glow: [f32; 4],
    /// Contrast, saturation, unused, unused.
    grade: [f32; 4],
    /// The area the pass draws and reads, in target pixels: left, top,
    /// right and bottom (exclusive).
    area: [f32; 4],
}

impl Look {
    /// The look for the four levels (glow, sharpen, contrast, saturation;
    /// 0 is off, 10 the strongest) on a frame `height` pixels tall, or
    /// `None` with every level off: the Classic picture. The glow's radius
    /// follows the frame so it reads the same on a 1080p and a 4K window.
    pub(crate) fn from_levels(levels: [u8; 4], height: u32, area: [u32; 4]) -> Option<Self> {
        if levels == [0; 4] || area[2] == 0 || area[3] == 0 {
            return None;
        }
        let [glow, sharpen, contrast, saturation] = levels.map(|level| f32::from(level.min(10)));
        let radius = (height as f32 / 1080.0 * 18.0).clamp(8.0, 48.0);
        Some(Self {
            glow: [glow * 0.28, 0.55, radius, sharpen * 0.09],
            grade: [1.0 + contrast * 0.016, 1.0 + saturation * 0.044, 0.0, 0.0],
            area: [
                area[0] as f32,
                area[1] as f32,
                (area[0] + area[2]) as f32,
                (area[1] + area[3]) as f32,
            ],
        })
    }

    /// The scissor of the area, clamped to a `width` by `height` target.
    fn scissor(&self, width: u32, height: u32) -> Option<[u32; 4]> {
        let left = (self.area[0] as u32).min(width);
        let top = (self.area[1] as u32).min(height);
        let right = (self.area[2] as u32).min(width);
        let bottom = (self.area[3] as u32).min(height);
        (right > left && bottom > top).then_some([left, top, right - left, bottom - top])
    }
}

fn shader_source() -> String {
    format!(
        "{}\n{}",
        super::webgl::srgb_wgsl(),
        include_str!("modern_graphics.wgsl")
    )
}

/// GPU resources derived from the frame's format and extent. A resize replaces
/// them; the mutex lets every present path share one pass through `&GpuContext`.
struct Resources {
    group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    scratch: wgpu::Texture,
    format: wgpu::TextureFormat,
    extent: wgpu::Extent3d,
}

pub(crate) struct ModernGraphicsPass {
    resources: Mutex<Option<Resources>>,
}

impl ModernGraphicsPass {
    pub(crate) fn new() -> Self {
        Self {
            resources: Mutex::new(None),
        }
    }

    /// Redraw the finished frame in place with the Modern look.
    /// A texture copy comes first because wgpu forbids reading and writing the
    /// same texture in one pass; all screenshot and capture readbacks are
    /// encoded earlier in the command buffer and stay uncorrected.
    pub(crate) fn encode(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        look: Look,
    ) {
        self.ensure(device, target);
        let resources_guard = lock(&self.resources);
        let resources = resources_guard
            .as_ref()
            .expect("modern graphics resources match the target");
        queue.write_buffer(&resources.uniform, 0, bytemuck::bytes_of(&look));
        encoder.copy_texture_to_texture(
            target.as_image_copy(),
            resources.scratch.as_image_copy(),
            resources.extent,
        );
        let view = target.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Modern graphics"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
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
        let Some([x, y, w, h]) = look.scissor(resources.extent.width, resources.extent.height)
        else {
            return;
        };
        pass.set_scissor_rect(x, y, w, h);
        pass.set_pipeline(&resources.pipeline);
        pass.set_bind_group(0, &resources.group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn ensure(&self, device: &wgpu::Device, target: &wgpu::Texture) {
        let format = target.format();
        let extent = target.size();
        let mut cached = lock(&self.resources);
        if cached
            .as_ref()
            .is_some_and(|resources| resources.format == format && resources.extent == extent)
        {
            return;
        }

        let scratch = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Modern graphics snapshot"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let source_view = scratch.create_view(&Default::default());

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Modern graphics"),
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
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Modern graphics"),
            size: std::mem::size_of::<Look>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Modern graphics"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&source_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Modern graphics"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Modern graphics"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Modern graphics"),
            layout: Some(&pipeline_layout),
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
                    format,
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

        *cached = Some(Resources {
            group,
            pipeline,
            uniform,
            scratch,
            format,
            extent,
        });
    }
}

/// The frame-output level owner of the pass. The frame loop publishes the
/// look once per frame; `None` is Classic and skips all GPU work.
pub(crate) struct ModernGraphics {
    look: Mutex<Option<Look>>,
    pass: ModernGraphicsPass,
}

impl ModernGraphics {
    pub(crate) fn new() -> Self {
        Self {
            look: Mutex::new(None),
            pass: ModernGraphicsPass::new(),
        }
    }

    /// Published by the frame loop: `Some` only for a match's frames.
    pub(crate) fn set_look(&self, look: Option<Look>) {
        *lock(&self.look) = look;
    }

    pub(crate) fn encode(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
    ) {
        let Some(look) = *lock(&self.look) else {
            return;
        };
        self.pass.encode(device, queue, encoder, target, look);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_graphics_shader_compiles() {
        use wgpu::naga;

        let source = shader_source();
        let module =
            naga::front::wgsl::parse_str(&source).expect("the Modern graphics shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("the Modern graphics shader validates");
    }

    #[test]
    fn the_glow_radius_follows_the_frame_height() {
        let area = [0, 0, 1920, 1080];
        let radius = |height| Look::from_levels([5; 4], height, area).unwrap().glow[2];
        assert_eq!((radius(1080), radius(2160), radius(200)), (18.0, 36.0, 8.0));
        // Every level off is Classic: no pass at all.
        assert_eq!(Look::from_levels([0; 4], 1080, area), None);
        // No battlefield to draw: no pass.
        assert_eq!(Look::from_levels([5; 4], 1080, [0, 0, 0, 1080]), None);
        // Contrast and saturation alone leave the glow and the sharpen off.
        let grade = Look::from_levels([0, 0, 10, 10], 1080, area).unwrap();
        assert_eq!((grade.glow[0], grade.glow[3]), (0.0, 0.0));
    }
}
