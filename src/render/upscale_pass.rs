//! Zoom of the match frame.
//!
//! A zoomed match draws its complete frame (battlefield, sidebar, messages,
//! tooltips, software cursor, and the active-game shells such as the pause
//! menu) into a source target the window size divided by the zoom; this pass
//! then scales it to the window, centred. A whole zoom (200%, 300%) copies
//! each source pixel as a block and stays sharp. A step between (150%) blends
//! the two source pixels a window pixel straddles by how much of it each
//! covers, so only the seams soften and no pixel is drawn wider than its
//! neighbours. A window that doesn't divide evenly keeps a black border
//! narrower than one source pixel. With the original 4:3 shape the frame is
//! the largest 4:3 rectangle that fits, centred between black bars, so the
//! pass runs even at 100%. The front-end menus never come here: they fit the
//! window on their own.
//!
//! Depends on: `render/gpu.rs` (GpuContext), `render/scaled_copy.rs` (the copy)
//! and the Zoom setting's geometry (`ui::main_menu_dialogs::options::ZoomGeometry`).

use super::gpu::GpuContext;
use super::scaled_copy::{
    MaskSource, MaskedCopy, ScaledSurface, WindowMask, create_color_target, create_depth_target,
};
use crate::ui::main_menu_dialogs::options::ZoomGeometry;

/// VERA-only wheel zoom composes at one source pixel per world pixel (stock YR
/// has no world zoom).
/// Only the completed scene is resampled; indexed palettes, native Z and
/// destination-dependent edits therefore never filter indices or depth values.
/// Screen-fixed UI is added to `color_view` afterwards at the frame resolution.
///
/// When the Options zoom also scales the frame, the frame's world would be
/// resampled twice: into the frame, then with the frame into the window. The
/// window copy (`draw_window_copy`) resamples the world once more, straight
/// into the window at both scales, over every frame pixel nothing drew on
/// after the world went in: a zoomed-out 2560x1440 window then shows all
/// 1920x1080 world pixels of the Huge view, not 1280x720 of them doubled.
pub(crate) struct WorldZoomPass {
    surface: ScaledSurface,
    world_depth: wgpu::TextureView,
    source_size: [u32; 2],
    scale: f32,
    /// The frame's battlefield as `composite` left it, before anything drew
    /// over it: the window copy's mask. Kept only while there is a window
    /// copy (`set_view`).
    world_frame: Option<wgpu::Texture>,
    window: MaskedCopy,
    window_placement: wgpu::Buffer,
}

/// The world target for `target` at `scale`: `ceil(target / scale)`, clamped
/// to the device's 2D texture limit (the minimum-zoom clamp on a very wide
/// window can otherwise round one texel past it).
fn world_source_size(device: &wgpu::Device, target: [u32; 2], scale: f32) -> [u32; 2] {
    let max = device.limits().max_texture_dimension_2d;
    target.map(|v| ((v as f32 / scale).ceil().max(1.0) as u32).min(max))
}

impl WorldZoomPass {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        target: [u32; 2],
        scale: f32,
    ) -> Self {
        let source_size = world_source_size(device, target, scale);
        let (_, world_depth) =
            create_depth_target(device, "Wheel zoom world", (source_size[0], source_size[1]));
        let surface = ScaledSurface::new(
            device,
            "Wheel zoom frame",
            format,
            (target[0], target[1]),
            Some(wgpu::TextureFormat::Depth32Float),
        );
        surface.place(queue, (0.0, 0.0), scale, scale.fract() == 0.0);
        let window = MaskedCopy::new(device, "Wheel zoom window", format, WindowMask::Changed);
        let window_placement = window.placement_buffer(device);
        Self {
            surface,
            world_depth,
            source_size,
            scale,
            world_frame: None,
            window,
            window_placement,
        }
    }

    /// `window_copy`: the Options zoom scales this frame into the window, so
    /// the world also goes there itself (`draw_window_copy`).
    pub(crate) fn set_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: [u32; 2],
        scale: f32,
        window_copy: bool,
    ) {
        let source_size = world_source_size(device, target, scale);
        if source_size != self.source_size {
            let (_, depth) =
                create_depth_target(device, "Wheel zoom world", (source_size[0], source_size[1]));
            self.world_depth = depth;
            self.source_size = source_size;
        }
        self.surface.resize(device, (target[0], target[1]));
        self.world_frame = window_copy.then(|| match self.world_frame.take() {
            Some(frame) if [frame.width(), frame.height()] == target.map(|v| v.max(1)) => frame,
            _ => {
                create_color_target(
                    device,
                    "Wheel zoom world frame",
                    self.surface.color_texture().format(),
                    (target[0], target[1]),
                )
                .0
            }
        });
        if scale != self.scale {
            self.surface
                .place(queue, (0.0, 0.0), scale, scale.fract() == 0.0);
            self.scale = scale;
        }
    }

    pub(crate) fn source_size(&self) -> [u32; 2] {
        self.source_size
    }
    pub(crate) fn depth_view(&self) -> &wgpu::TextureView {
        &self.world_depth
    }
    pub(crate) fn color_view(&self) -> &wgpu::TextureView {
        self.surface.color_view()
    }
    pub(crate) fn color_texture(&self) -> &wgpu::Texture {
        self.surface.color_texture()
    }

    pub(crate) fn frame_depth_view(&self) -> &wgpu::TextureView {
        self.surface.depth_view()
    }

    pub(crate) fn world_clip(&self, [x, y, w, h]: [u32; 4]) -> [u32; 4] {
        let left = (x as f32 / self.scale).floor() as u32;
        let top = (y as f32 / self.scale).floor() as u32;
        let right = (((x + w) as f32 / self.scale).ceil() as u32).min(self.source_size[0]);
        let bottom = (((y + h) as f32 / self.scale).ceil() as u32).min(self.source_size[1]);
        [
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        ]
    }

    /// Resample the completed world `source` into the frame's `tactical`
    /// rect. With a window copy, also keep that rect as it is now, before
    /// anything draws over it, for `draw_window_copy`.
    pub(crate) fn composite(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Texture,
        tactical: [u32; 4],
        clear: wgpu::Color,
    ) {
        let bind_group = self.surface.bind_group_for(device, source);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Wheel zoom completed world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: self.surface.color_view(),
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: self.surface.depth_view(),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(super::native_z::STORED_DEPTH_CLEAR),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.surface.draw(&mut pass, Some(&bind_group), tactical);
        drop(pass);
        let Some(world_frame) = &self.world_frame else {
            return;
        };
        let frame = world_frame.size();
        let [x, y, width, height] = tactical;
        let (x, y) = (x.min(frame.width), y.min(frame.height));
        let (width, height) = (width.min(frame.width - x), height.min(frame.height - y));
        if width > 0 && height > 0 {
            let at = |texture| wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            };
            encoder.copy_texture_to_texture(
                at(self.surface.color_texture()),
                at(world_frame),
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    /// Draw the world `source` into `view`, a window that `zoom` has just
    /// scaled this frame into, at the zoom's scale times the wheel's, over
    /// the window pixels wholly inside the frame's `tactical` rect. A window
    /// pixel over a frame pixel that changed since `composite` kept the world
    /// keeps the zoomed frame. Draws nothing unless `set_view` kept a window
    /// copy.
    pub(crate) fn draw_window_copy(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        source: &wgpu::Texture,
        zoom: ZoomGeometry,
        tactical: (u32, u32, u32, u32),
    ) {
        let Some(world_frame) = &self.world_frame else {
            return;
        };
        // World pixel (0, 0) is frame pixel (0, 0) (`new`), which the zoom
        // puts at its origin.
        let origin = (zoom.origin.0 as f32, zoom.origin.1 as f32);
        let scale = zoom.scale() * self.scale;
        self.window.place(
            queue,
            &self.window_placement,
            origin,
            scale,
            scale.fract() == 0.0,
        );
        self.window.place_frame(queue, origin, zoom.scale());
        let source = source.create_view(&Default::default());
        let before = world_frame.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Wheel zoom window"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
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
        self.window.draw(
            device,
            &mut pass,
            &source,
            &self.window_placement,
            MaskSource::Changed {
                frame: self.surface.color_view(),
                before: &before,
            },
            zoom.window_rect(tactical),
        );
    }
}

/// GPU resources for the zoom pass.
pub struct UpscalePass {
    surface: ScaledSurface,
    geometry: ZoomGeometry,
}

impl UpscalePass {
    /// Create a pass that zooms a `geometry.src_width x src_height` frame into
    /// the window it was computed for.
    pub fn new(gpu: &GpuContext, geometry: ZoomGeometry) -> Self {
        let surface = ScaledSurface::new(
            &gpu.device,
            "Zoom",
            gpu.surface_format,
            (geometry.src_width, geometry.src_height),
            None,
        );
        place(&surface, gpu, geometry);
        Self { surface, geometry }
    }

    pub fn geometry(&self) -> ZoomGeometry {
        self.geometry
    }

    /// Follow a new window size or zoom.
    pub fn set_geometry(&mut self, gpu: &GpuContext, geometry: ZoomGeometry) {
        if geometry == self.geometry {
            return;
        }
        self.surface
            .resize(&gpu.device, (geometry.src_width, geometry.src_height));
        place(&self.surface, gpu, geometry);
        self.geometry = geometry;
    }

    /// Cached view for the intermediate color render target.
    pub fn color_view(&self) -> &wgpu::TextureView {
        self.surface.color_view()
    }

    /// Intermediate color texture receiving the completed tactical frame.
    pub fn color_texture(&self) -> &wgpu::Texture {
        self.surface.color_texture()
    }

    /// Cached view for the intermediate depth buffer.
    pub fn depth_view(&self) -> &wgpu::TextureView {
        self.surface.depth_view()
    }

    /// Source render width.
    pub fn src_width(&self) -> u32 {
        self.geometry.src_width
    }

    /// Source render height.
    pub fn src_height(&self) -> u32 {
        self.geometry.src_height
    }

    /// Zoom the intermediate texture into `target_view`, a window-sized target.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target_view: &wgpu::TextureView) {
        self.draw_from(encoder, target_view, None);
    }

    /// Draw the same zoom from an explicitly retained source.
    /// Used only for a requested screenshot of the previously presented
    /// cursor-free composition; ordinary frames keep the persistent bind group.
    pub(crate) fn draw_texture(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Texture,
        target_view: &wgpu::TextureView,
    ) {
        let bind_group = self.surface.bind_group_for(device, source);
        self.draw_from(encoder, target_view, Some(&bind_group));
    }

    fn draw_from(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        source: Option<&wgpu::BindGroup>,
    ) {
        // The window outside the frame is black.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Zoom Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
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
        let (x, y) = self.geometry.origin;
        let (width, height) = self.geometry.frame_size();
        self.surface.draw(&mut pass, source, [x, y, width, height]);
    }
}

fn place(surface: &ScaledSurface, gpu: &GpuContext, geometry: ZoomGeometry) {
    surface.place(
        &gpu.queue,
        (geometry.origin.0 as f32, geometry.origin.1 as f32),
        geometry.scale(),
        geometry.is_sharp(),
    );
}

#[cfg(all(test, tests_render_s_z))]
mod tests {
    use super::*;
    use crate::ui::main_menu_dialogs::options::FrameShape;

    #[test]
    #[ignore = "requires GPU; the wheel-zoomed world reaches a zoomed window in one resample"]
    fn wheel_zoomed_world_reaches_a_zoomed_window_in_one_resample() {
        use crate::render::terrain_draw_gpu_tests::Gpu;
        const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
        let gpu = Gpu::new();
        let fill = |texture: &wgpu::Texture, texel: &dyn Fn(u32, u32) -> [u8; 4]| {
            let (width, height) = (texture.width(), texture.height());
            let bytes: Vec<u8> = (0..height)
                .flat_map(|y| (0..width).flat_map(move |x| texel(x, y)))
                .collect();
            gpu.queue.write_texture(
                texture.as_image_copy(),
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: None,
                },
                texture.size(),
            );
        };
        // Options zoom 200% puts an 8x8 window's frame at 4x4; wheel zoom
        // 0.5 shows an 8x8 world in it: one world pixel per window pixel.
        let zoom = ZoomGeometry::new(8, 8, 200, FrameShape::Window);
        assert_eq!((zoom.src_width, zoom.src_height), (4, 4));
        let mut pass = WorldZoomPass::new(&gpu.device, &gpu.queue, FORMAT, [4, 4], 0.5);
        pass.set_view(&gpu.device, &gpu.queue, [4, 4], 0.5, true);
        assert_eq!(pass.source_size(), [8, 8]);
        // A one-pixel checkerboard, which the 4x4 frame can only show grey.
        let checker = |x: u32, y: u32| {
            if (x + y) % 2 == 0 {
                [255; 4]
            } else {
                [0, 0, 0, 255]
            }
        };
        let world = gpu.target([8, 8], FORMAT);
        fill(&world, &checker);
        // The zoomed frame as the zoom pass left it in the window.
        let blue = [0, 0, 255, 255];
        let window = gpu.target([8, 8], FORMAT);
        fill(&window, &|_, _| blue);
        let red = gpu.target([1, 1], FORMAT);
        fill(&red, &|_, _| [255, 0, 0, 255]);

        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        pass.composite(
            &gpu.device,
            &mut encoder,
            &world,
            [0, 0, 4, 4],
            wgpu::Color::BLACK,
        );
        // UI the frame then draws over the battlefield, at frame pixel (3, 3).
        encoder.copy_texture_to_texture(
            red.as_image_copy(),
            wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d { x: 3, y: 3, z: 0 },
                ..pass.color_texture().as_image_copy()
            },
            red.size(),
        );
        pass.draw_window_copy(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            &window.create_view(&Default::default()),
            &world,
            zoom,
            (0, 0, 4, 4),
        );
        let reads = [gpu.read(&mut encoder, &window)];
        let output = gpu.finish(encoder, &reads, [8, 8]).remove(0);
        for y in 0..8 {
            for x in 0..8 {
                let expected = if x >= 6 && y >= 6 {
                    blue
                } else {
                    checker(x, y)
                };
                let at = ((y * 8 + x) * 4) as usize;
                assert_eq!(output[at..at + 4], expected, "window pixel {x},{y}");
            }
        }
    }
}
