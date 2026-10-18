//! Bracket, LaserDraw, EBolt, Wave and LineTrail edits on the shared snapshot/Z.
//! Per-pixel operations retain registry/segment/raster order. Cross-pixel
//!4BC750 pairs begin a dependency chunk when their inputs have prior writes;
//!independent pairs share a snapshot/resolve, never a pass per pixel.

use super::TerrainDrawRenderer;
use crate::render::line_trail::{LineTrailSegment, rasterize};
use crate::render::surface_line::{
    SurfaceDraw, SurfaceLine, SurfaceLineBlend, SurfaceLineViewport,
};

/// A frame-local range in the shared line upload. Display parents and the
/// later Tactical effects pass replay their own ranges against the live Z.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct SurfaceLineBatch {
    start: usize,
    end: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Operation {
    z: u32,
    strength: i32,
    rgb: u32,
    alpha: u32,
    mode: u32,
    destination_weight: i32,
    gate_x: i32,
    gate_y: i32,
    source_x: i32,
    source_y: i32,
}

// Ten scalars:40-byte storage-array stride in Rust and WGSL, without hidden
// vec2 padding. https://www.w3.org/TR/WGSL/#alignment-and-size
const _: () = assert!(std::mem::size_of::<Operation>() == 40);

fn packed_rgb(rgb: [u8; 3]) -> u32 {
    u32::from(rgb[0]) | (u32::from(rgb[1]) << 8) | (u32::from(rgb[2]) << 16)
}

fn raster_draw(
    draw: SurfaceDraw,
    viewport: SurfaceLineViewport,
    size: [u32; 2],
    alpha: &mut impl FnMut([i32; 2]) -> u16,
    emit: &mut impl FnMut(&[([i32; 2], Operation)]),
) -> Result<(), crate::render::wave_geometry::MagneticRasterError> {
    use crate::render::surface_line::{antialiased_line, ellipse, rasterize_z_clipped};
    let line = match draw {
        SurfaceDraw::MagneticWave(wave) => {
            let logical_size = size.map(|v| (v as f32 / viewport.zoom).ceil() as i32);
            // Native exactness is zoom1. Other zooms expand the same logical
            // pixels and sample the corresponding logical pixel's center.
            wave.rasterize(
                viewport.clip,
                logical_size,
                logical_size[0],
                viewport.z_origin_y,
                |pixel| {
                    let source = pixel
                        .source
                        .map(|v| ((v as f32 + 0.5) * viewport.zoom).floor() as i32);
                    emit(&[(
                        pixel.point,
                        Operation {
                            z: u32::from(pixel.candidate_z),
                            strength: pixel.brightness,
                            mode: 7,
                            source_x: source[0],
                            source_y: source[1],
                            ..Default::default()
                        },
                    )]);
                },
            )?;
            return Ok(());
        }
        SurfaceDraw::Point(point) => {
            let screen = point.screen.to_array();
            let a = alpha(screen);
            if a != 0 {
                let word = crate::render::tactical_compat::pack_rgb(
                    crate::render::tactical_compat::modulate_rgb(point.rgb, a),
                    crate::render::native_surface_format::RGB565,
                );
                emit(&[(
                    screen,
                    Operation {
                        // Keep the signed candidate intact across the scalar ABI.
                        z: point.candidate_z as u32,
                        rgb: u32::from(word),
                        alpha: u32::from(a),
                        mode: 6,
                        ..Default::default()
                    },
                )]);
            }
            return Ok(());
        }
        SurfaceDraw::Ellipse {
            center,
            radius,
            word,
        } => {
            ellipse(center, radius, viewport.clip, |point| {
                emit(&[(
                    point,
                    Operation {
                        rgb: u32::from(word),
                        mode: 4,
                        ..Default::default()
                    },
                )]);
            });
            return Ok(());
        }
        SurfaceDraw::Line(line) => line,
    };
    if let SurfaceLineBlend::Antialiased { rgb, intensity } = line.blend {
        // Selected456DD8 always passes equal -500 adjustments. Other native
        // parameter branches remain explicit residuals until a caller ports them.
        assert_eq!(line.z_adjust[0], line.z_adjust[1]);
        antialiased_line(
            line.from,
            line.to,
            line.z_adjust[0],
            intensity,
            viewport.clip,
            size.map(|v| (v as f32 / viewport.zoom).ceil() as i32),
            viewport.z_origin_y,
            |pair| {
                let a = alpha(pair[0].primary);
                if a == 0 {
                    return;
                }
                // A pair must use the SAME primary depth sample. Sample the
                // logical primary's center after presentation zoom, never the
                // secondary output's depth. Native parity is zoom1.
                let gate = pair[0]
                    .primary
                    .map(|v| ((v as f32 + 0.5) * viewport.zoom).floor() as i32);
                let operations = pair.map(|pixel| {
                    let source = pixel
                        .destination_source
                        .map(|v| ((v as f32 + 0.5) * viewport.zoom).floor() as i32);
                    (
                        pixel.point,
                        Operation {
                            z: u32::from(pixel.z),
                            strength: pixel.source_weight,
                            rgb: packed_rgb(rgb),
                            alpha: u32::from(a),
                            mode: if pixel.destination_source == pixel.point {
                                3
                            } else {
                                5
                            },
                            destination_weight: pixel.destination_weight,
                            gate_x: gate[0],
                            gate_y: gate[1],
                            source_x: source[0],
                            source_y: source[1],
                        },
                    )
                });
                emit(&operations);
            },
        );
        return Ok(());
    }
    let (rgb, mode) = match line.blend {
        SurfaceLineBlend::Add(rgb) => (packed_rgb(rgb), 1),
        SurfaceLineBlend::Replace(word) => (u32::from(word), 2),
        SurfaceLineBlend::Antialiased { .. } => unreachable!(),
    };
    rasterize_z_clipped(
        line.from,
        line.to,
        line.z_adjust,
        viewport.clip,
        viewport.z_origin_y,
        |pixel| {
            //4BDF00 advances A only with Y; packed4BFD30 advances both axes.
            let sample = if mode == 1 {
                [pixel.clipped_start_x, pixel.point[1]]
            } else {
                pixel.point
            };
            let a = alpha(sample);
            if a != 0 {
                emit(&[(
                    pixel.point,
                    Operation {
                        z: u32::from(pixel.z),
                        rgb,
                        alpha: u32::from(a),
                        mode,
                        ..Default::default()
                    },
                )]);
            }
        },
    );
    Ok(())
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
    /// Derived solely from work's output points; clear on every upload,
    ///prepare and begin. Bounds cross-pixel dependencies, not display state.
    written: std::collections::HashSet<[i32; 2]>,
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
            written: Default::default(),
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
            // Snapshot the shared primary gate as well as output coverage.
            //Cross-source pairs remain atomic across every chunk boundary.
            for (_, operation) in &self.work[start..end] {
                if operation.mode == 3 || operation.mode == 5 {
                    left = left.min(operation.gate_x.max(0) as u32);
                    top = top.min(operation.gate_y.max(0) as u32);
                    right = right.max(operation.gate_x.saturating_add(1).max(0) as u32);
                    bottom = bottom.max(operation.gate_y.saturating_add(1).max(0) as u32);
                }
                if operation.mode == 7 {
                    //7601FB reads a displaced old word, which can lie outside
                    //this chunk's output rect. Snapshot its actual address,
                    //not an invented clamped neighbor or stale scratch word.
                    left = left.min(operation.source_x.max(0) as u32);
                    top = top.min(operation.source_y.max(0) as u32);
                    right = right.max(operation.source_x.saturating_add(1).max(0) as u32);
                    bottom = bottom.max(operation.source_y.saturating_add(1).max(0) as u32);
                }
            }
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
        self.written.clear();
    }
}

impl TerrainDrawRenderer {
    #[cfg(test)]
    pub(crate) fn set_surface_line_operation_limit(&mut self, limit: Option<usize>) {
        self.surface_lines.test_operation_limit = limit;
    }

    /// Recycle the derived upload ranges before preparing this composite.
    pub(crate) fn begin_surface_lines(&mut self) {
        self.surface_lines.active = 0;
        self.surface_lines.work.clear();
        self.surface_lines.written.clear();
    }

    /// Prepare only actual raster operations; empty frames allocate/upload nothing.
    /// Native exactness is zoom1. Other zooms expand the same logical pixels.
    pub(crate) fn prepare_surface_lines(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        lasers: impl IntoIterator<Item = crate::render::laser::LaserDraw>,
        draws: impl Iterator<Item = SurfaceDraw> + Clone,
        segments: &[LineTrailSegment],
        post_trail_draws: &[SurfaceDraw],
        radials: &[crate::render::radial_indicator::RadialIndicatorDraw],
        viewport: SurfaceLineViewport,
        mut high_detail: impl FnMut(crate::render::laser::LaserDrawColor) -> bool,
        mut alpha: impl FnMut([i32; 2]) -> u16,
    ) -> SurfaceLineBatch {
        let start = self.surface_lines.active;
        self.surface_lines.work.clear();
        self.surface_lines.written.clear();
        let Some(targets) = &self.targets else {
            return SurfaceLineBatch::default();
        };
        let size = [targets.source_color.width(), targets.source_color.height()];
        let limit = (device.limits().max_storage_buffer_binding_size as usize
            / std::mem::size_of::<Operation>())
        .min(1_048_576);
        assert!(limit >= 2, "the native pair must fit one storage binding");
        #[cfg(test)]
        let limit = self
            .surface_lines
            .test_operation_limit
            .unwrap_or(limit)
            .clamp(1, limit);
        // Only4BC750 pairs can read a neighboring output. Ordinary object
        // extras and Laser/EBolt/Trail calls need no prior-write index. When
        // rays are possible, index every earlier write in this same batch.
        let track_dependencies = radials.iter().any(|draw| draw.rays)
            || draws
                .clone()
                .chain(post_trail_draws.iter().copied())
                .any(|draw| {
                    matches!(
                        draw,
                        SurfaceDraw::Line(SurfaceLine {
                            blend: SurfaceLineBlend::Antialiased { .. },
                            ..
                        })
                    )
                });
        // Tactical6D4669..6D4673 draws LaserDraw, EBolt, then LineTrail.
        // They read the same immutable Z and completed destination. Laser
        // colors and Bolt subdivision consumed Main for this composite;
        // rasterization, upload chunks and readback cannot request more draws.
        let store = |gpu: &mut SurfaceLineGpu, operations: &[([i32; 2], Operation)]| {
            //4BD646/68F read BOTH old words before4BD85B/8C0 write either.
            //A cross-source pair cannot read another operation's in-chunk
            //result. Flush only its prior-write dependency or the size limit;
            //all independent pairs/ordinary operations share the upload.
            let dependency = track_dependencies
                && operations
                    .iter()
                    .any(|(point, op)| op.mode == 5 && gpu.written.contains(point));
            if dependency || gpu.work.len() + operations.len() > limit.max(operations.len()) {
                gpu.upload(device, queue, viewport.zoom, size);
            }
            for &(point, operation) in operations {
                gpu.work.push((point, operation));
                if track_dependencies {
                    gpu.written.insert(point);
                }
            }
        };
        let prepare_draw = |gpu: &mut SurfaceLineGpu, draw: SurfaceDraw, alpha: &mut _| {
            let magnetic = matches!(draw, SurfaceDraw::MagneticWave(_));
            if magnetic {
                // Even adjacent coalesced Surface bodies need the preceding
                //Wave/object's completed destination, never a frame snapshot.
                gpu.upload(device, queue, viewport.zoom, size);
            }
            let first = gpu.active;
            if let Err(error) = raster_draw(draw, viewport, size, alpha, &mut |operations| {
                store(gpu, operations)
            }) {
                // Roll back this whole Wave, including any size-driven
                //uploads. Unsupported raw native memory domains are an
                //explicit residual, not a clamp, partial beam or game crash.
                gpu.active = first;
                gpu.work.clear();
                gpu.written.clear();
                log::warn!("Wave3 unsupported native raster domain {error:?}: {draw:?}");
            }
            if magnetic {
                // Native traversal is retained before size-driven chunks.
                //Self/future-only reads cannot observe an earlier same-Wave
                //write even when a chunk starts a fresh snapshot.
                gpu.upload(device, queue, viewport.zoom, size);
            }
        };
        for laser in lasers {
            if laser.duration > 0 {
                laser.lines(viewport.camera, high_detail(laser.color), |line| {
                    prepare_draw(&mut self.surface_lines, SurfaceDraw::Line(line), &mut alpha);
                });
            }
        }
        for draw in draws {
            prepare_draw(&mut self.surface_lines, draw, &mut alpha);
        }
        for &segment in segments {
            rasterize(
                segment.project(viewport.camera),
                viewport.clip,
                viewport.z_origin_y,
                |pixel| {
                    let a = alpha(pixel.point);
                    if a != 0 {
                        store(
                            &mut self.surface_lines,
                            &[(
                                pixel.point,
                                Operation {
                                    z: u32::from(pixel.z),
                                    strength: segment.strength,
                                    rgb: packed_rgb(segment.color),
                                    alpha: u32::from(a),
                                    mode: 0,
                                    ..Default::default()
                                },
                            )],
                        );
                    }
                },
            );
        }
        //Tactical6D4678 draws RadBeam after trails and before selected radials.
        for &draw in post_trail_draws {
            prepare_draw(&mut self.surface_lines, draw, &mut alpha);
        }
        //Tactical6D467F follows Laser/EBolt/LineTrail/RadBeam, before the bandbox,
        //second rally and selected Foot action. Keep each object's ellipse/ray
        //stores interleaved in CurrentObjects encounter order.
        for &radial in radials {
            radial.draws(viewport.clip, |draw| {
                prepare_draw(&mut self.surface_lines, draw, &mut alpha)
            });
        }
        self.surface_lines
            .upload(device, queue, viewport.zoom, size);
        SurfaceLineBatch {
            start,
            end: self.surface_lines.active,
        }
    }

    /// Replay this range at its native phase: line stores consume the current
    /// destination and perform their own ABuffer multiplication.
    /// Reuses the SAME snapshot and live depth as ordinary tactical drawing.
    pub(crate) fn draw_surface_lines(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        batch: SurfaceLineBatch,
    ) {
        let Some(targets) = &self.targets else {
            return;
        };
        assert!(batch.end <= self.surface_lines.active);
        for batch in &self.surface_lines.batches[batch.start..batch.end] {
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

#[cfg(test)]
#[path = "electric_bolt_tests.rs"]
mod electric_bolt_tests;

#[cfg(test)]
#[path = "radiation_beam_gpu_tests.rs"]
mod radiation_beam_tests;

#[cfg(test)]
#[path = "radial_indicator_gpu_tests.rs"]
mod radial_indicator_tests;

#[cfg(test)]
#[path = "welding_gpu_tests.rs"]
mod welding_tests;
