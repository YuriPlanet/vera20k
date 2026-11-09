//! Whole-viewport daltonization for colour-vision deficiencies.
//!
//! VERA-only presentation extension: gamemd has no equivalent, nothing here
//! reaches `sim/`, the network, a save or a replay, so clients may differ
//! without a desync. The pass is the last draw of the frame, so it covers the
//! game world, the sidebar, menus, overlays and the software cursor together.
//!
//! The matrices come from open-northland PR #45 (offered for reuse without
//! attribution) and are recorded in `docs/color-vision-config.md`. They were
//! calibrated on gamma-encoded sRGB values through an SVG `feColorMatrix` with
//! `color-interpolation-filters="sRGB"`. Both the native and the WebGL2 path
//! bind ordinary sRGB views and convert to/from encoded bytes in the shader;
//! no reinterpret view is needed, so the two builds take the same numeric path.
//! `Off` skips the pass entirely, leaving the retail image byte-identical.

use std::sync::{Mutex, MutexGuard, PoisonError};

use bytemuck::{Pod, Zeroable};

/// A poisoned lock still holds valid GPU handles; a panic elsewhere must not
/// turn a presentation-only filter into a second failure.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) type Mat3 = [[f32; 3]; 3];

const SIM_DEUTERANOPIA: Mat3 = [
    [0.625, 0.375, 0.0],
    [0.700, 0.300, 0.0],
    [0.0, 0.300, 0.700],
];
const SIM_PROTANOPIA: Mat3 = [
    [0.567, 0.433, 0.0],
    [0.558, 0.442, 0.0],
    [0.0, 0.242, 0.758],
];
const SIM_TRITANOPIA: Mat3 = [
    [0.950, 0.050, 0.0],
    [0.0, 0.433, 0.567],
    [0.0, 0.475, 0.525],
];
const SHIFT_RED_GREEN: Mat3 = [[0.0, 0.0, 0.0], [0.7, 1.0, 0.0], [0.7, 0.0, 1.0]];
const SHIFT_BLUE_YELLOW: Mat3 = [[1.0, 0.0, 0.7], [0.0, 1.0, 0.7], [0.0, 0.0, 0.0]];

/// `M_correct = M_sim + M_shift * (I - M_sim)`: the simulation matrix loses
/// part of the colour information, and the shift matrix routes that lost error
/// into a channel the deficient eye can still separate. Const so every mode's
/// matrix is a compile-time constant rather than per-frame work.
pub(crate) const fn correction(sim: Mat3, shift: Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    let mut i = 0;
    while i < 3 {
        let mut j = 0;
        while j < 3 {
            let mut value = sim[i][j];
            let mut k = 0;
            while k < 3 {
                let lost = (if k == j { 1.0 } else { 0.0 }) - sim[k][j];
                value += shift[i][k] * lost;
                k += 1;
            }
            out[i][j] = value;
            j += 1;
        }
        i += 1;
    }
    out
}

pub(crate) const CORRECT_DEUTERANOPIA: Mat3 = correction(SIM_DEUTERANOPIA, SHIFT_RED_GREEN);
pub(crate) const CORRECT_PROTANOPIA: Mat3 = correction(SIM_PROTANOPIA, SHIFT_RED_GREEN);
pub(crate) const CORRECT_TRITANOPIA: Mat3 = correction(SIM_TRITANOPIA, SHIFT_BLUE_YELLOW);

/// Three vec4 rows: WGSL uniform layout pads every row to 16 bytes. The w
/// component is unused; the fragment shader only reads xyz.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CorrectionUniform {
    rows: [[f32; 4]; 3],
}

impl CorrectionUniform {
    fn new(matrix: Mat3) -> Self {
        Self {
            rows: [
                [matrix[0][0], matrix[0][1], matrix[0][2], 0.0],
                [matrix[1][0], matrix[1][1], matrix[1][2], 0.0],
                [matrix[2][0], matrix[2][1], matrix[2][2], 0.0],
            ],
        }
    }
}

fn shader_source() -> String {
    format!(
        "{}\n{}",
        super::webgl::srgb_wgsl(),
        include_str!("color_vision.wgsl")
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

pub(crate) struct ColorVisionPass {
    resources: Mutex<Option<Resources>>,
}

impl ColorVisionPass {
    pub(crate) fn new() -> Self {
        Self {
            resources: Mutex::new(None),
        }
    }

    /// Recolour the finished frame in place. The caller skips this for `Off`.
    /// A texture copy comes first because wgpu forbids reading and writing the
    /// same texture in one pass; all screenshot and capture readbacks are
    /// encoded earlier in the command buffer and stay uncorrected.
    pub(crate) fn encode(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        matrix: Mat3,
    ) {
        self.ensure(device, target);
        let resources_guard = lock(&self.resources);
        let resources = resources_guard
            .as_ref()
            .expect("colour-vision resources match the target");
        queue.write_buffer(
            &resources.uniform,
            0,
            bytemuck::bytes_of(&CorrectionUniform::new(matrix)),
        );
        encoder.copy_texture_to_texture(
            target.as_image_copy(),
            resources.scratch.as_image_copy(),
            resources.extent,
        );
        let view = target.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Color vision"),
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
            label: Some("Color vision snapshot"),
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
            label: Some("Color vision"),
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
            label: Some("Color vision"),
            size: std::mem::size_of::<CorrectionUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Color vision"),
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
            label: Some("Color vision"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Color vision"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Color vision"),
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

/// The frame-output level owner of the filter. The frame loop publishes the
/// player's matrix once per frame; every present path — including the
/// synchronous loading repaints that submit outside the frame loop — encodes
/// from that same value. `None` is `Off` and skips all GPU work.
pub(crate) struct ColorVisionFilter {
    matrix: Mutex<Option<Mat3>>,
    pass: ColorVisionPass,
}

impl ColorVisionFilter {
    pub(crate) fn new() -> Self {
        Self {
            matrix: Mutex::new(None),
            pass: ColorVisionPass::new(),
        }
    }

    /// Published by the frame loop; loading repaints reuse the last value
    /// because the player cannot open Options while a map load is running.
    pub(crate) fn set_matrix(&self, matrix: Option<Mat3>) {
        *lock(&self.matrix) = matrix;
    }

    pub(crate) fn encode(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
    ) {
        let Some(matrix) = *lock(&self.matrix) else {
            return;
        };
        self.pass.encode(device, queue, encoder, target, matrix);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_matrix(actual: Mat3, expected: Mat3) {
        for (row, wanted) in actual.iter().zip(expected) {
            for (value, wanted) in row.iter().zip(wanted) {
                assert!(
                    (value - wanted).abs() < 1.0e-5,
                    "matrix row differs: {actual:?} vs {expected:?}"
                );
            }
        }
    }

    /// `Off` is the untouched image, and the three modes expand to the
    /// reference daltonization matrices (each row sums to 1, so white stays
    /// white). A VERA-only pure function needs no native oracle or golden.
    #[test]
    fn correction_matrices_match_the_reference_values() {
        // No information lost means no correction, whatever the shift table.
        let identity: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        expect_matrix(correction(identity, SHIFT_RED_GREEN), identity);
        expect_matrix(
            CORRECT_DEUTERANOPIA,
            [
                [0.625, 0.375, 0.0],
                [0.2625, 0.7375, 0.0],
                [0.2625, -0.2625, 1.0],
            ],
        );
        expect_matrix(
            CORRECT_PROTANOPIA,
            [
                [0.567, 0.433, 0.0],
                [0.3031, 0.6969, 0.0],
                [0.3031, -0.3031, 1.0],
            ],
        );
        expect_matrix(
            CORRECT_TRITANOPIA,
            [
                [1.0, -0.3325, 0.3325],
                [0.0, 0.6675, 0.3325],
                [0.0, 0.475, 0.525],
            ],
        );
        for matrix in [CORRECT_DEUTERANOPIA, CORRECT_PROTANOPIA, CORRECT_TRITANOPIA] {
            for row in matrix {
                assert!(
                    (row.iter().sum::<f32>() - 1.0).abs() < 1.0e-5,
                    "white point moved: {row:?}"
                );
            }
        }
    }

    #[test]
    fn color_vision_shader_compiles() {
        use wgpu::naga;
        let source = shader_source();
        let module =
            naga::front::wgsl::parse_str(&source).expect("the colour-vision shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("the colour-vision shader validates");
    }

    /// Synchronous loading repaints present outside the frame loop. Any new
    /// direct present path must encode the shared filter too, or the screen
    /// briefly returns to uncorrected colours during a map load.
    #[test]
    fn every_direct_present_path_encodes_the_shared_filter() {
        // `concat!` keeps this test's own source from matching itself if the
        // scan is ever widened to include `src/render`.
        const PRESENT: &str = concat!("output.present", "(");
        const ENCODE: &str = concat!("encode_color_vision", "(");

        fn visit(dir: &std::path::Path, checked: &mut usize) {
            for entry in std::fs::read_dir(dir).expect("read source directory") {
                let path = entry.expect("read source entry").path();
                if path.is_dir() {
                    visit(&path, checked);
                    continue;
                }
                if !path.extension().is_some_and(|ext| ext == "rs") {
                    continue;
                }
                let source = std::fs::read_to_string(&path).expect("read source file");
                let lines: Vec<&str> = source
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("//"))
                    .collect();
                let presents = lines.iter().filter(|line| line.contains(PRESENT)).count();
                if presents == 0 {
                    continue;
                }
                // A file with two presents but one encode is exactly the
                // regression this guards, so compare counts, not presence.
                let encodes = lines.iter().filter(|line| line.contains(ENCODE)).count();
                assert!(
                    encodes >= presents,
                    "{} presents {presents} frame(s) but encodes the shared colour-vision filter only {encodes} time(s)",
                    path.display()
                );
                *checked += presents;
            }
        }

        let mut checked = 0;
        visit(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app"),
            &mut checked,
        );
        assert!(
            checked >= 5,
            "expected the frame loop and the loading repaints, found {checked}"
        );
    }
}
