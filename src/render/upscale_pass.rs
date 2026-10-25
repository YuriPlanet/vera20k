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

use super::gpu::GpuContext;
use super::scaled_copy::ScaledSurface;

/// The match frame's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrameShape {
    /// The window's own: a wider window shows more battlefield.
    #[default]
    Window,
    /// gamemd's 4:3 screen, with black bars on a window of any other shape.
    Original,
}

/// Where a zoomed frame sits in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoomGeometry {
    /// Window pixels per 100 source pixels, on each axis; at least 100.
    pub percent: u32,
    pub src_width: u32,
    pub src_height: u32,
    /// The window pixel where source pixel (0, 0) lands.
    pub origin: (u32, u32),
}

impl ZoomGeometry {
    pub fn new(window_width: u32, window_height: u32, percent: u32, shape: FrameShape) -> Self {
        let percent = percent.max(100);
        let mut src_width = (window_width * 100 / percent).max(1);
        let mut src_height = (window_height * 100 / percent).max(1);
        if shape == FrameShape::Original {
            if src_width * 3 > src_height * 4 {
                src_width = (src_height * 4 / 3).max(1);
            } else {
                src_height = (src_width * 3 / 4).max(1);
            }
        }
        let zoom = Self {
            percent,
            src_width,
            src_height,
            origin: (0, 0),
        };
        let (frame_width, frame_height) = zoom.frame_size();
        Self {
            origin: (
                window_width.saturating_sub(frame_width) / 2,
                window_height.saturating_sub(frame_height) / 2,
            ),
            ..zoom
        }
    }

    /// Window pixels per source pixel.
    pub fn scale(self) -> f32 {
        self.percent as f32 / 100.0
    }

    /// Whether every source pixel is a whole block of window pixels.
    pub fn is_sharp(self) -> bool {
        self.percent.is_multiple_of(100)
    }

    /// The scaled frame, in whole window pixels.
    pub fn frame_size(self) -> (u32, u32) {
        (
            self.src_width * self.percent / 100,
            self.src_height * self.percent / 100,
        )
    }

    /// Whether the frame is the whole window, unscaled: no pass is needed.
    pub fn is_whole_window(self, window_width: u32, window_height: u32) -> bool {
        self.percent == 100 && (self.src_width, self.src_height) == (window_width, window_height)
    }

    /// A window point in source pixels. The border clamps to the frame's edge.
    pub fn window_to_source(self, x: f32, y: f32) -> (f32, f32) {
        let scale = self.scale();
        (
            ((x - self.origin.0 as f32) / scale).clamp(0.0, (self.src_width - 1) as f32),
            ((y - self.origin.1 as f32) / scale).clamp(0.0, (self.src_height - 1) as f32),
        )
    }

    /// A source point in window pixels.
    pub fn source_to_window(self, x: f32, y: f32) -> (f32, f32) {
        let scale = self.scale();
        (
            x * scale + self.origin.0 as f32,
            y * scale + self.origin.1 as f32,
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_centres_whole_blocks_and_maps_points_both_ways() {
        // An even window: the frame covers it exactly.
        let wide = ZoomGeometry::new(3440, 1440, 200, FrameShape::Window);
        assert_eq!(
            (wide.src_width, wide.src_height, wide.origin),
            (1720, 720, (0, 0))
        );
        // An odd window keeps a border under one block, split evenly.
        let odd = ZoomGeometry::new(1925, 1083, 300, FrameShape::Window);
        assert_eq!(
            (odd.src_width, odd.src_height, odd.origin),
            (641, 361, (1, 0))
        );
        assert_eq!(odd.source_to_window(10.0, 20.0), (31.0, 60.0));
        assert_eq!(odd.window_to_source(31.0, 60.0), (10.0, 20.0));
        // The border clamps to the frame's edge pixels.
        assert_eq!(odd.window_to_source(0.0, 1082.0), (0.0, 360.0));
        // 100% is the identity.
        let plain = ZoomGeometry::new(800, 600, 100, FrameShape::Window);
        assert_eq!(
            (plain.src_width, plain.src_height, plain.origin),
            (800, 600, (0, 0))
        );
        assert_eq!(plain.window_to_source(399.5, 299.5), (399.5, 299.5));
        assert!(plain.is_whole_window(800, 600));
        assert!(!wide.is_whole_window(3440, 1440));
        assert!(wide.is_sharp() && odd.is_sharp());
    }

    #[test]
    fn a_step_between_whole_zooms_scales_smoothly_from_the_centre() {
        let half = ZoomGeometry::new(1920, 1080, 150, FrameShape::Window);
        assert_eq!(
            (half.src_width, half.src_height, half.origin),
            (1280, 720, (0, 0))
        );
        assert!(!half.is_sharp());
        assert_eq!(half.frame_size(), (1920, 1080));
        assert_eq!(half.source_to_window(10.0, 20.0), (15.0, 30.0));
        assert_eq!(half.window_to_source(15.0, 30.0), (10.0, 20.0));
        // A window the step doesn't divide keeps a border under one source
        // pixel.
        let odd = ZoomGeometry::new(1366, 768, 150, FrameShape::Window);
        assert_eq!((odd.src_width, odd.src_height), (910, 512));
        assert_eq!((odd.frame_size(), odd.origin), ((1365, 768), (0, 0)));
        assert!(!odd.is_whole_window(1366, 768));
    }

    #[test]
    fn original_shape_keeps_a_4_3_frame_between_centred_bars() {
        let frame = |w, h, percent| {
            let zoom = ZoomGeometry::new(w, h, percent, FrameShape::Original);
            (zoom.src_width, zoom.src_height, zoom.origin)
        };
        // A widescreen gets bars at the sides, at any zoom.
        assert_eq!(frame(1920, 1080, 100), (1440, 1080, (240, 0)));
        assert_eq!(frame(1920, 1080, 200), (720, 540, (240, 0)));
        assert_eq!(frame(1920, 1080, 150), (960, 720, (240, 0)));
        // A taller screen gets them above and below.
        assert_eq!(frame(1280, 1024, 100), (1280, 960, (0, 32)));
        // A 4:3 window is unchanged, and at 100% needs no pass.
        assert_eq!(frame(1600, 1200, 200), (800, 600, (0, 0)));
        let whole = ZoomGeometry::new(1024, 768, 100, FrameShape::Original);
        assert!(whole.is_whole_window(1024, 768));
        let barred = ZoomGeometry::new(1920, 1080, 100, FrameShape::Original);
        assert!(!barred.is_whole_window(1920, 1080));
        // The bars clamp to the frame's edge, like the zoom's own border.
        assert_eq!(barred.window_to_source(100.0, 500.0), (0.0, 500.0));
        assert_eq!(barred.source_to_window(0.0, 0.0), (240.0, 0.0));
    }
}
