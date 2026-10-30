//! `IonBlastClass::DrawAll @ 0x0053D850`'s ripple (per blast `0x0053D580`) as
//! destination edits on TerrainDrawRenderer's snapshot and live Z.
//!
//! The app hands over the blasts DrawAll draws, last first, with each frame's
//! screen origin ([`IonBlastDraw`]). A blast's 512x256 frame is clipped to the
//! tactical view less its bottom 7 rows (`0x0053D5B0..0x0053D5E5`,
//! `XSurface::Prep_For_Blit @ 0x007BC040`). A pixel whose byte is positive and
//! whose native Z lies above the row's threshold
//! `low16(seed - AdjustForZ(z) - y - 3)` (`0x0053D6A2..0x0053D6B9`,
//! `0x0053D72F`; the seed is the ZBuffer's `+0x24`, here
//! `native_z::DEFAULT_Z`) takes the destination pixel its byte's move names
//! ([`displacement`]). Native edits in
//! place, top row first. Every byte the generator writes moves a pixel up from
//! a row below, which that blast has not written yet, so reading a snapshot
//! taken before the blast gives the same pixels. A blast reads what the
//! previous one left: one snapshot and one edit pass per blast.
//!
//! The app generates the frames once, at start-up, as native does
//! ([`TerrainDrawRenderer::generate_ion_blast_frames`]).
//!
//! Native exactness is zoom 1; other zooms expand the same logical pixels. A
//! move from the clip's last rows reads up to 3 rows below the tactical view
//! (the clip stops 7 rows short and a move reaches 10). Here those rows of the
//! composition target still hold the black clear colour; native's Composite
//! surface is screen-high, zero-filled by `Allocate_Surfaces @ 0x00533FD0`,
//! and the tactical draws clip it out (read, not executed).
//!
//! Evidence: `tools/superweapon_oracle.py` section `ion_blast_draw` runs the
//! original DrawAll on fixture surfaces;
//! `app/presentation/ion_blasts_gpu_tests.rs` draws its cases through the
//! production admission and this owner.

use super::TerrainDrawRenderer;
use crate::sim::ion_blast_ripple::{
    FRAME_COUNT, FRAME_HEIGHT, FRAME_WIDTH, RippleFrames, displacement,
};

/// One blast DrawAll draws, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IonBlastDraw {
    /// The frame's top-left: the blast's point less (256, 128).
    pub origin: [i32; 2],
    /// The tactical view less its bottom 7 rows: x, y, width, height.
    pub clip: [i32; 4],
    /// The blast's frame, the ripple frame it draws.
    pub frame: u32,
    /// `low16(0x8000 - AdjustForZ(z))`, the threshold of row 3 above row 0.
    pub z_seed: u16,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlastInstance {
    quad: [f32; 4],
    origin: [i32; 2],
    frame_seed: [u32; 2],
    zoom: f32,
}

/// One blast's scissors in target pixels: the pixels it may write and the
/// pixels its moves may read.
struct Prepared {
    edit: [u32; 4],
    read: [u32; 4],
}

pub(super) struct IonBlastGpu {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// The frames and moves.
    binding: Option<wgpu::BindGroup>,
    instances: Option<wgpu::Buffer>,
    instance_capacity: usize,
    staged: Vec<BlastInstance>,
    prepared: Vec<Prepared>,
    /// The farthest a move reaches, in pixels across and rows.
    reach: [i32; 2],
}

impl IonBlastGpu {
    pub(super) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        snapshot: &wgpu::BindGroupLayout,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("IonBlast ripple frames and moves"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
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
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("IonBlast ripple"),
            source: wgpu::ShaderSource::Wgsl(
                crate::render::tactical_shader::source(include_str!("terrain_ion_blast.wgsl"))
                    .into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("IonBlast shared snapshot layout"),
            bind_group_layouts: &[Some(snapshot), Some(&layout)],
            immediate_size: 0,
        });
        let attributes =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Sint32x2, 2 => Uint32x2, 3 => Float32];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("IonBlast ripple destination edit"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<BlastInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                })],
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
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let reach = (1..=u8::MAX)
            .filter_map(displacement)
            .fold([0, 0], |reach, [x, y]| {
                [reach[0].max(x.abs()), reach[1].max(y.abs())]
            });
        Self {
            pipeline,
            layout,
            binding: None,
            instances: None,
            instance_capacity: 0,
            staged: Vec::new(),
            prepared: Vec::new(),
            reach,
        }
    }

    /// `frames` ([`FRAME_COUNT`] frames of bytes) and the moves.
    fn bind_frames(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frames: &[u8]) {
        let size = wgpu::Extent3d {
            width: FRAME_WIDTH as u32,
            height: FRAME_HEIGHT as u32,
            depth_or_array_layers: FRAME_COUNT as u32,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("IonBlast ripple frames"),
            size,
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
            frames,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(FRAME_WIDTH as u32),
                rows_per_image: Some(FRAME_HEIGHT as u32),
            },
            size,
        );
        let moves: Vec<[i32; 4]> = (0..128u8)
            .map(|byte| displacement(byte).map_or([0; 4], |[x, y]| [x, y, 0, 0]))
            .collect();
        let moves = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("IonBlast ripple moves"),
                contents: bytemuck::cast_slice(&moves),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        self.binding = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("IonBlast ripple frames and moves"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: moves.as_entire_binding(),
                },
            ],
        }));
    }
}

impl TerrainDrawRenderer {
    /// The ripple frames, which take about 75 ms to generate. Native generates
    /// them once per process at start-up: `Init_Game` (`0x0052C946`) ->
    /// `Show_Loading_Screen` (`0x00531758`) -> `0x0053D330`, gated by
    /// `0x00AA014C`.
    pub(crate) fn generate_ion_blast_frames(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let frames = RippleFrames::shared();
        self.ion_blasts.bind_frames(device, queue, frames.bytes());
    }

    /// DrawAll's blasts for this frame, in its order. A frame without one
    /// uploads nothing.
    pub(crate) fn prepare_ion_blasts(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        draws: &[IonBlastDraw],
        zoom: f32,
    ) {
        let gpu = &mut self.ion_blasts;
        gpu.staged.clear();
        gpu.prepared.clear();
        let (Some(targets), Some(_)) = (&self.targets, &gpu.binding) else {
            return;
        };
        let size = [targets.source_color.width(), targets.source_color.height()];
        let target = |logical: i32, limit: u32, round: fn(f32) -> f32| {
            (round(logical as f32 * zoom).max(0.0) as u32).min(limit)
        };
        for draw in draws {
            let [x, y, width, height] = draw.clip;
            let left = draw.origin[0].max(x);
            let top = draw.origin[1].max(y);
            let right = (draw.origin[0] + FRAME_WIDTH as i32).min(x + width);
            let bottom = (draw.origin[1] + FRAME_HEIGHT as i32).min(y + height);
            if left >= right || top >= bottom {
                continue;
            }
            let edit = [
                target(left, size[0], f32::floor),
                target(top, size[1], f32::floor),
                target(right, size[0], f32::ceil),
                target(bottom, size[1], f32::ceil),
            ];
            if edit[0] >= edit[2] || edit[1] >= edit[3] {
                continue;
            }
            let [across, rows] = gpu.reach;
            let read = [
                target(left - across, size[0], f32::floor),
                target(top - rows, size[1], f32::floor),
                target(right + across, size[0], f32::ceil),
                target(bottom + rows, size[1], f32::ceil),
            ];
            gpu.staged.push(BlastInstance {
                quad: [
                    left as f32 * zoom / size[0] as f32 * 2.0 - 1.0,
                    1.0 - top as f32 * zoom / size[1] as f32 * 2.0,
                    (right - left) as f32 * zoom / size[0] as f32 * 2.0,
                    -((bottom - top) as f32) * zoom / size[1] as f32 * 2.0,
                ],
                origin: draw.origin,
                frame_seed: [draw.frame, u32::from(draw.z_seed)],
                zoom,
            });
            let span = |[x0, y0, x1, y1]: [u32; 4]| [x0, y0, x1 - x0, y1 - y0];
            gpu.prepared.push(Prepared {
                edit: span(edit),
                read: span(read),
            });
        }
        if gpu.staged.is_empty() {
            return;
        }
        if gpu.instance_capacity < gpu.staged.len() {
            gpu.instance_capacity = gpu.staged.len().next_power_of_two();
            gpu.instances = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("IonBlast ripple blasts"),
                size: (gpu.instance_capacity * std::mem::size_of::<BlastInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(instances) = &gpu.instances {
            queue.write_buffer(instances, 0, bytemuck::cast_slice(&gpu.staged));
        }
    }

    /// Replace the generated frames with fixture ones.
    #[cfg(test)]
    pub(crate) fn bind_ion_blast_frames_for_test(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frames: &[u8],
    ) {
        self.ion_blasts.bind_frames(device, queue, frames);
    }

    /// Whether this frame draws a ripple.
    pub(crate) fn has_ion_blasts(&self) -> bool {
        !self.ion_blasts.prepared.is_empty()
    }

    /// Each blast's snapshot and edit, in DrawAll's order. The caller has
    /// ended its pass over the same attachments.
    pub(crate) fn draw_ion_blasts(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
    ) {
        let Some(targets) = &self.targets else {
            return;
        };
        let gpu = &self.ion_blasts;
        let (Some(binding), Some(instances)) = (&gpu.binding, &gpu.instances) else {
            return;
        };
        let attachment = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })
        };
        for (index, blast) in gpu.prepared.iter().enumerate() {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("IonBlast shared color/Z snapshot"),
                    color_attachments: &[attachment(&targets.words), attachment(&targets.depth)],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                let [x, y, w, h] = blast.read;
                pass.set_scissor_rect(x, y, w, h);
                pass.set_pipeline(&self.snapshot_pipeline);
                pass.set_bind_group(0, &targets.source, &[]);
                pass.draw(0..3, 0..1);
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("IonBlast ripple"),
                    color_attachments: &[attachment(color)],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                let [x, y, w, h] = blast.edit;
                pass.set_scissor_rect(x, y, w, h);
                pass.set_pipeline(&gpu.pipeline);
                pass.set_bind_group(0, &targets.snapshot, &[]);
                pass.set_bind_group(1, binding, &[]);
                pass.set_vertex_buffer(0, instances.slice(..));
                let index = index as u32;
                pass.draw(0..6, index..index + 1);
            }
            self.submission.note_closed_passes(encoder, 2);
        }
    }
}
