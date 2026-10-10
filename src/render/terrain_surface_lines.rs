//! LaserDraw and LineTrail destination edits on TerrainDrawRenderer's existing snapshot/Z.
//! Different pixels are independent; every repeated operation for one pixel
//! retains the original registry/segment/raster order. One snapshot and resolve
//! per bounded chunk avoid a render pass per line or pixel.

use super::TerrainDrawRenderer;
use crate::render::line_trail::{LineTrailSegment, rasterize};
use crate::render::surface_line::SurfaceLineViewport;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Operation {
    z: u32,
    strength: i32,
    rgb: u32,
    alpha: u32,
    mode: u32,
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PixelInstance {
    rect: [f32; 4],
    span: [u32; 2],
}
struct Prepared {
    operations: wgpu::Buffer,
    instances: wgpu::Buffer,
    binding: wgpu::BindGroup,
    operation_capacity: usize,
    instance_capacity: usize,
    count: u32,
    scissor: [u32; 4],
}

pub(super) struct SurfaceLineGpu {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    batches: Vec<Prepared>,
    active: usize,
    #[cfg(test)]
    test_operation_limit: Option<usize>,
    work: Vec<([i32; 2], Operation)>,
    operations: Vec<Operation>,
    instances: Vec<PixelInstance>,
}

impl SurfaceLineGpu {
    pub(super) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        snapshot: &wgpu::BindGroupLayout,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Surface line ordered pixel operations"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Original surface line RGB565 operations"),
            source: wgpu::ShaderSource::Wgsl(
                crate::render::tactical_shader::source(include_str!("terrain_surface_lines.wgsl"))
                    .into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Surface line shared snapshot layout"),
            bind_group_layouts: &[snapshot, &layout],
            push_constant_ranges: &[],
        });
        let attrs = wgpu::vertex_attr_array![0=>Float32x4,1=>Uint32x2];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Surface line ordered destination resolution"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<PixelInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attrs,
                }],
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
            multiview: None,
            cache: None,
        });
        Self {
            pipeline,
            layout,
            batches: Vec::new(),
            active: 0,
            #[cfg(test)]
            test_operation_limit: None,
            work: Vec::new(),
            operations: Vec::new(),
            instances: Vec::new(),
        }
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, zoom: f32, size: [u32; 2]) {
        if self.work.is_empty() {
            return;
        }
        // Stable sort is a dependency grouping, never a reordering at a pixel.
        self.work.sort_by_key(|(point, _)| (point[1], point[0]));
        self.operations.clear();
        self.instances.clear();
        let mut start = 0;
        let mut left = size[0];
        let mut top = size[1];
        let mut right = 0;
        let mut bottom = 0;
        while start < self.work.len() {
            let point = self.work[start].0;
            let mut end = start + 1;
            while end < self.work.len() && self.work[end].0 == point {
                end += 1;
            }
            self.operations
                .extend(self.work[start..end].iter().map(|(_, op)| *op));
            let x = point[0] as f32 * zoom;
            let y = point[1] as f32 * zoom;
            self.instances.push(PixelInstance {
                rect: [
                    x / size[0] as f32 * 2.0 - 1.0,
                    1.0 - y / size[1] as f32 * 2.0,
                    zoom / size[0] as f32 * 2.0,
                    -zoom / size[1] as f32 * 2.0,
                ],
                span: [start as u32, (end - start) as u32],
            });
            left = left.min(x.floor().max(0.) as u32);
            top = top.min(y.floor().max(0.) as u32);
            right = right.max((x + zoom).ceil().max(0.) as u32);
            bottom = bottom.max((y + zoom).ceil().max(0.) as u32);
            start = end;
        }
        let index = self.active;
        let op_capacity = self.operations.len().next_power_of_two();
        let instance_capacity = self.instances.len().next_power_of_two();
        let limit = device.limits().max_storage_buffer_binding_size as usize
            / std::mem::size_of::<Operation>();
        if self.batches.get(index).is_none_or(|b| {
            b.operation_capacity < self.operations.len()
                || b.instance_capacity < self.instances.len()
        }) {
            let op_capacity = op_capacity.min(limit);
            let operations = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Surface line ordered operations"),
                size: (op_capacity * std::mem::size_of::<Operation>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Surface line touched pixels"),
                size: (instance_capacity * std::mem::size_of::<PixelInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Surface line pixel operation binding"),
                layout: &self.layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: operations.as_entire_binding(),
                }],
            });
            let prepared = Prepared {
                operations,
                instances,
                binding,
                operation_capacity: op_capacity,
                instance_capacity,
                count: 0,
                scissor: [0; 4],
            };
            if index == self.batches.len() {
                self.batches.push(prepared);
            } else {
                self.batches[index] = prepared;
            }
        }
        let batch = &mut self.batches[index];
        queue.write_buffer(&batch.operations, 0, bytemuck::cast_slice(&self.operations));
        queue.write_buffer(&batch.instances, 0, bytemuck::cast_slice(&self.instances));
        right = right.min(size[0]);
        bottom = bottom.min(size[1]);
        batch.count = self.instances.len() as u32;
        batch.scissor = [
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        ];
        self.active += 1;
        self.work.clear();
    }
}

impl TerrainDrawRenderer {
    /// Prepare only actual raster operations; empty frames allocate/upload nothing.
    /// Native exactness is zoom1. Other zooms expand the same logical pixels.
    pub(crate) fn prepare_surface_lines(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        lasers: impl IntoIterator<Item = crate::render::laser::LaserDraw>,
        segments: &[LineTrailSegment],
        viewport: SurfaceLineViewport,
        mut high_detail: impl FnMut() -> bool,
        mut alpha: impl FnMut([i32; 2]) -> u16,
    ) {
        self.surface_lines.active = 0;
        self.surface_lines.work.clear();
        let Some(targets) = &self.targets else {
            return;
        };
        let size = [targets.source_color.width(), targets.source_color.height()];
        let limit = (device.limits().max_storage_buffer_binding_size as usize
            / std::mem::size_of::<Operation>())
        .min(1_048_576);
        #[cfg(test)]
        let limit = self
            .surface_lines
            .test_operation_limit
            .unwrap_or(limit)
            .clamp(1, limit);
        assert!(limit > 0);
        // Tactical6D4669 draws LaserDraw before LineTrail6D4673. They read
        // the same immutable Z and completed destination, so one ordered
        // operation stream preserves both stages without another snapshot.
        for laser in lasers {
            if laser.duration <= 0 {
                continue;
            }
            laser.lines(viewport.camera, high_detail(), |line| {
                let (rgb, mode) = match line.blend {
                    crate::render::laser::LaserBlend::Add(rgb) => (
                        u32::from(rgb[0]) | (u32::from(rgb[1]) << 8) | (u32::from(rgb[2]) << 16),
                        1,
                    ),
                    crate::render::laser::LaserBlend::Replace(word) => (u32::from(word), 2),
                };
                crate::render::surface_line::rasterize_z_clipped(
                    line.from,
                    line.to,
                    line.z_adjust,
                    viewport.clip,
                    viewport.z_origin_y,
                    |pixel| {
                        // Original additive4BDF00 advances the A pointer
                        // only on Y changes (4BE6FA), never on X (4BE769).
                        // Packed4BFD30 advances both axes like LineTrail.
                        let sample = if mode == 1 {
                            [pixel.clipped_start_x, pixel.point[1]]
                        } else {
                            pixel.point
                        };
                        let a = alpha(sample);
                        if a == 0 {
                            return;
                        }
                        self.surface_lines.work.push((
                            pixel.point,
                            Operation {
                                z: u32::from(pixel.z),
                                strength: 0,
                                rgb,
                                alpha: u32::from(a),
                                mode,
                            },
                        ));
                        if self.surface_lines.work.len() == limit {
                            self.surface_lines
                                .upload(device, queue, viewport.zoom, size);
                        }
                    },
                );
            });
        }
        for &segment in segments {
            let projected = segment.project(viewport.camera);
            let rgb = u32::from(segment.color[0])
                | (u32::from(segment.color[1]) << 8)
                | (u32::from(segment.color[2]) << 16);
            rasterize(projected, viewport.clip, viewport.z_origin_y, |pixel| {
                let a = alpha(pixel.point);
                if a == 0 {
                    return;
                }
                self.surface_lines.work.push((
                    pixel.point,
                    Operation {
                        z: u32::from(pixel.z),
                        strength: segment.strength,
                        rgb,
                        alpha: u32::from(a),
                        mode: 0,
                    },
                ));
                if self.surface_lines.work.len() == limit {
                    self.surface_lines
                        .upload(device, queue, viewport.zoom, size);
                }
            });
        }
        self.surface_lines
            .upload(device, queue, viewport.zoom, size);
    }

    /// Draw after the app's global shroud multiply: native lines consume an
    /// already-shaded destination and perform their own ABuffer multiplication.
    /// Reuses the SAME snapshot and live depth as ordinary tactical drawing.
    pub(crate) fn draw_surface_lines(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
    ) {
        let Some(targets) = &self.targets else {
            return;
        };
        for batch in &self.surface_lines.batches[..self.surface_lines.active] {
            let [x, y, w, h] = batch.scissor;
            if w == 0 || h == 0 {
                continue;
            }
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
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Surface line shared color/Z snapshot"),
                    color_attachments: &[attachment(&targets.words), attachment(&targets.depth)],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_scissor_rect(x, y, w, h);
                pass.set_pipeline(&self.snapshot_pipeline);
                pass.set_bind_group(0, &targets.source, &[]);
                pass.draw(0..3, 0..1);
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Surface line original ordered blends"),
                    color_attachments: &[attachment(color)],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_scissor_rect(x, y, w, h);
                pass.set_pipeline(&self.surface_lines.pipeline);
                pass.set_bind_group(0, &targets.snapshot, &[]);
                pass.set_bind_group(1, &batch.binding, &[]);
                pass.set_vertex_buffer(0, batch.instances.slice(..));
                pass.draw(0..6, 0..batch.count);
            }
            self.submission.note_closed_passes(encoder, 2);
        }
    }
}

#[cfg(test)]
#[path = "line_trail_gpu_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "laser_tests.rs"]
mod laser_tests;
