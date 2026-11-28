//! The match frame in the window.
//!
//! A match lays out its frame (battlefield, sidebar, messages, cursor) in
//! frame pixels: the window size divided by the Options zoom, in the window's
//! shape or the original 4:3 (`ZoomGeometry`). The battlefield and the frame's
//! interface are drawn straight at the window's pixels, into a composition
//! the size of the frame's rect in the window (`ZoomGeometry::frame_size`):
//! the world camera's zoom is the wheel zoom times the Options scale, and the
//! UI camera scales frame pixels by the Options scale
//! (`BatchRenderer::update_match_camera`, `docs/zoom-one-pass.md`). This pass
//! holds that composition's depth ([`UpscalePass::depth_view`]) and puts the
//! finished composition in the window, centred between black bars
//! ([`UpscalePass::present`]). With the original 4:3 shape the frame is the
//! largest 4:3 rectangle that fits, so the pass runs even at 100%. The
//! front-end menus never come here: they fit the window on their own.
//!
//! The active-game shells (the pause menu and the like) still paint at frame
//! pixels into the pass's source target, and the pass scales that to the
//! window ([`UpscalePass::draw`]): a whole zoom (200%, 300%) copies each
//! source pixel as a block and stays sharp; a step between (150%) blends the
//! two source pixels a window pixel straddles by how much of it each covers,
//! so only the seams soften. A shell that paints over the retained match
//! composition takes it back at frame pixels ([`UpscalePass::shrink`]).
//!
//! Depends on: `render/gpu.rs` (GpuContext), `render/scaled_copy.rs` (the copy)
//! and the Zoom setting's geometry (`ui::main_menu_dialogs::options::ZoomGeometry`).

use super::gpu::GpuContext;
use super::scaled_copy::{ScaledSurface, create_depth_target};
use crate::ui::main_menu_dialogs::options::ZoomGeometry;

/// GPU resources for the zoom pass.
pub struct UpscalePass {
    /// The active-game shells' frame, at frame pixels, scaled into the window.
    surface: ScaledSurface,
    /// The copy that takes a window-pixel composition back to frame pixels.
    /// Its own 1x1 target is never drawn; it copies other textures only.
    shrink: ScaledSurface,
    /// Depth for the match composition, the frame's rect in the window.
    // Kept with its view for GPU resource ownership.
    _composition_depth_texture: wgpu::Texture,
    composition_depth: wgpu::TextureView,
    geometry: ZoomGeometry,
}

impl UpscalePass {
    /// Create a pass for `geometry`'s frame in the window it was computed
    /// for.
    pub fn new(gpu: &GpuContext, geometry: ZoomGeometry) -> Self {
        let surface = ScaledSurface::new(
            &gpu.device,
            "Zoom",
            gpu.surface_format,
            (geometry.src_width, geometry.src_height),
            None,
        );
        let shrink =
            ScaledSurface::new(&gpu.device, "Zoom shrink", gpu.surface_format, (1, 1), None);
        let (_composition_depth_texture, composition_depth) =
            create_depth_target(&gpu.device, "Match composition", geometry.frame_size());
        let pass = Self {
            surface,
            shrink,
            _composition_depth_texture,
            composition_depth,
            geometry,
        };
        pass.place(gpu);
        pass
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
        if geometry.frame_size() != self.geometry.frame_size() {
            (self._composition_depth_texture, self.composition_depth) =
                create_depth_target(&gpu.device, "Match composition", geometry.frame_size());
        }
        self.geometry = geometry;
        self.place(gpu);
    }

    /// The shells' source texture, at frame pixels.
    pub fn color_texture(&self) -> &wgpu::Texture {
        self.surface.color_texture()
    }

    /// Depth for the match composition: the frame's rect in the window.
    pub fn depth_view(&self) -> &wgpu::TextureView {
        &self.composition_depth
    }

    /// Source render width.
    pub fn src_width(&self) -> u32 {
        self.geometry.src_width
    }

    /// Source render height.
    pub fn src_height(&self) -> u32 {
        self.geometry.src_height
    }

    /// Zoom the shells' source texture into `target_view`, a window-sized
    /// target.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target_view: &wgpu::TextureView) {
        self.draw_from(encoder, target_view, None);
    }

    /// Zoom a retained frame-pixel `source` (an active-game shell's
    /// cursor-free composition) the same way. Used only for a requested
    /// screenshot or a modal over it; ordinary frames keep the persistent
    /// bind group.
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

    /// Put `source`, a match composition at the window's pixels (the frame's
    /// rect there), into the window-sized `destination` at the frame's
    /// origin, with black outside it. A whole-pixel copy: nothing resamples.
    pub(crate) fn present(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Texture,
        destination: &wgpu::Texture,
    ) {
        let view = destination.create_view(&Default::default());
        drop(clear_pass(encoder, &view, "Zoom bars"));
        let (x, y) = self.geometry.origin;
        let width = source.width().min(destination.width().saturating_sub(x));
        let height = source.height().min(destination.height().saturating_sub(y));
        if width == 0 || height == 0 {
            return;
        }
        encoder.copy_texture_to_texture(
            source.as_image_copy(),
            wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d { x, y, z: 0 },
                ..destination.as_image_copy()
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Draw `source`, a match composition at the window's pixels, into the
    /// frame-sized top-left corner of `destination` at frame pixels: the
    /// retained match under a shell that paints over it at frame pixels
    /// (`screenshot::PresentedFrameCache::encode_previous_composition`). The
    /// copy integrates the window pixels each frame pixel covers; the rest of
    /// `destination` is left as it is.
    pub(crate) fn shrink(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Texture,
        destination: &wgpu::Texture,
    ) {
        let bind_group = self.shrink.bind_group_for(device, source);
        let view = destination.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Zoom shrink"),
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
        let width = self.geometry.src_width.min(destination.width());
        let height = self.geometry.src_height.min(destination.height());
        self.shrink
            .draw(&mut pass, Some(&bind_group), [0, 0, width, height]);
    }

    fn draw_from(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        source: Option<&wgpu::BindGroup>,
    ) {
        // The window outside the frame is black.
        let mut pass = clear_pass(encoder, target_view, "Zoom Pass");
        let (x, y) = self.geometry.origin;
        let (width, height) = self.geometry.frame_size();
        self.surface.draw(&mut pass, source, [x, y, width, height]);
    }

    fn place(&self, gpu: &GpuContext) {
        let geometry = self.geometry;
        self.surface.place(
            &gpu.queue,
            (geometry.origin.0 as f32, geometry.origin.1 as f32),
            geometry.scale(),
            geometry.is_sharp(),
        );
        // At 100% (the 4:3 bars only) the shrink is a whole-pixel copy.
        self.shrink.place(
            &gpu.queue,
            (0.0, 0.0),
            1.0 / geometry.scale(),
            geometry.percent == 100,
        );
    }
}

/// A pass over `view` that clears it to black.
fn clear_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    label: &'static str,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
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
    })
}
