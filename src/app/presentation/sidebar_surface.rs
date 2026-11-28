//! The sidebar's own surfaces: gamemd's `g_SidebarSurface`, and the command
//! bar along the frame's bottom.
//!
//! Both draw in their own native pixels (`ui::sidebar::frame`): the sidebar
//! into a surface 168 pixels wide, the bar into one 32 pixels tall.
//! `SidebarClass::BlitToScreen` (`6A70E0`) puts the finished sidebar in the
//! frame's right-hand column; here that copy, and the bar's into the bottom
//! strip, also scale by the player's Sidebar size. Normal and Extra Large
//! copy 1:1 or in whole blocks; Large draws at its frame's 1.5 pixels and
//! copies 1:1, so a cameo its columns shrank is rasterized at the size it
//! shows rather than in a smaller cell scaled up afterwards.
//!
//! Drawing into the surfaces also keeps the sidebar's depth there: the frame's
//! column keeps the cleared depth, so the tooltips and the software cursor
//! drawn after the copy always show over the radar, as gamemd draws the mouse
//! last.
//!
//! A zoomed match draws the surfaces at the window's pixels instead
//! (`SidebarFrame::in_window`): each sidebar pixel a block of whole window
//! pixels. Their cameras stay in sidebar pixels, so every draw lands where
//! it did and covers its pixels' blocks. The match composition is at the
//! window's pixels too (`docs/zoom-one-pass.md`), so the copy puts them
//! there at full size, and the UI drawn after it goes over them.
//!
//! The column's text always draws at the window's pixels: in the surface
//! where its pixels are the window's, over the copy where they are not
//! ([`column_text`]).

use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::batch::{BatchRenderer, CameraUniform};
use crate::render::bit_font::TextRaster;
use crate::render::scaled_copy::ScaledSurface;
use crate::render::terrain_draw::{
    PackedCompositePiece, PackedSpriteKind, PackedSurface, TerrainDrawRenderer,
};
use crate::ui::game_screen::GameScreen;
use crate::ui::sidebar::SidebarFrame;

/// A part of the sidebar with a surface of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarPart {
    /// The sidebar itself: a column, or a band on the top or the bottom
    /// edge.
    Column,
    /// The command bar on its strip.
    Bar,
}

pub(crate) struct SidebarSurface {
    column: PartSurface,
    bar: PartSurface,
    frame: SidebarFrame,
    packed: Option<PackedSurface>,
    column_camera: CameraUniform,
    /// The column's camera at its surface's own pixels, for its text
    /// ([`column_text`]).
    text_camera_buffer: wgpu::Buffer,
    text_camera: wgpu::BindGroup,
    /// The UI camera [`Self::write_cameras`] last wrote.
    written_ui: Option<CameraUniform>,
    /// What the surfaces were last drawn from ([`Self::redraw_for`]).
    drawn: Option<(SidebarPicture, SidebarFrame, CameraUniform)>,
}

/// Every instance stream the sidebar's surfaces draw (`draw_passes`).
pub(crate) const SIDEBAR_STREAMS: [&str; 16] = [
    "sidebar_chrome",
    "sidebar_cameo",
    "sidebar_cameo_hover",
    "sidebar_unavailable",
    "sidebar_flash",
    "sidebar_queue_overlay",
    "sidebar_queue_text",
    "sidebar_label_overlay",
    "sidebar_label_text",
    "sidebar_gclock",
    "sidebar_cameo_overlay",
    "sidebar_text",
    "radar_anim",
    "minimap",
    "radar_content_boundary",
    "command_bar",
];

/// The stream of the radar's viewport outline, the one that moves with the
/// camera on the sidebar.
pub(crate) const VIEWPORT_OUTLINE: &str = "viewport_rect";

/// Every input of the sidebar's draw besides its frame and camera. Equal
/// inputs draw equal pixels, so the surfaces already hold them.
#[derive(PartialEq)]
pub(crate) struct SidebarPicture {
    /// The generation of each instance stream it draws.
    pub(crate) streams: Vec<u64>,
    /// The texture of each layer that has one.
    pub(crate) textures: Vec<Option<wgpu::BindGroup>>,
    /// The generations of the minimap's and the housing's pictures.
    pub(crate) radar: [u64; 2],
    /// The generation of the [`VIEWPORT_OUTLINE`], and whether it lies
    /// wholly on the minimap's quad.
    pub(crate) outline: u64,
    pub(crate) outline_on_minimap: bool,
    pub(crate) text_in_surface: bool,
}

/// How much of the surfaces a frame draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarRedraw {
    /// They already hold the picture.
    Nothing,
    /// Only the minimap's composition, or an outline that stays on it,
    /// changed. The quad is opaque, so only its pixels change, whatever the
    /// surface held there.
    Minimap,
    Whole,
}

/// One part's surface, and the camera the batch pipelines draw into it with.
struct PartSurface {
    surface: ScaledSurface,
    camera_buffer: wgpu::Buffer,
    camera: wgpu::BindGroup,
}

impl SidebarSurface {
    /// Surfaces for `frame` that the batch pipelines draw in `format`, copied
    /// into a frame pass with a Depth32Float attachment.
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        batch: &BatchRenderer,
        frame: SidebarFrame,
    ) -> Self {
        let part = |label, size| {
            let (camera_buffer, camera) = batch.create_ui_camera(device);
            PartSurface {
                surface: ScaledSurface::new(
                    device,
                    label,
                    format,
                    size,
                    Some(wgpu::TextureFormat::Depth32Float),
                ),
                camera_buffer,
                camera,
            }
        };
        let (text_camera_buffer, text_camera) = batch.create_ui_camera(device);
        let surface = Self {
            column: part("Sidebar Surface", frame.surface_draw_size()),
            bar: part("Command Bar Surface", frame.bar_draw_size()),
            frame,
            packed: None,
            column_camera: sidebar_ui(batch.ui_camera_uniform()),
            text_camera_buffer,
            text_camera,
            written_ui: None,
            drawn: None,
        };
        surface.place(queue);
        surface
    }

    /// Follow a new frame size or Sidebar size.
    pub(crate) fn set_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: SidebarFrame,
    ) {
        if frame == self.frame {
            return;
        }
        if frame.surface_draw_size() != self.frame.surface_draw_size() {
            self.packed = None;
        }
        self.column
            .surface
            .resize(device, frame.surface_draw_size());
        self.bar.surface.resize(device, frame.bar_draw_size());
        self.frame = frame;
        self.written_ui = None;
        self.place(queue);
    }

    /// What this frame must draw for `picture`, recording it as the one the
    /// surfaces will hold.
    pub(crate) fn redraw_for(&mut self, picture: SidebarPicture) -> SidebarRedraw {
        // Scrolling moves the streams and their camera together.
        let camera = CameraUniform {
            camera_pos: [0.0; 2],
            ..self.column_camera
        };
        let inputs = (picture, self.frame, camera);
        let redraw = match self.drawn.as_mut() {
            Some(drawn) if *drawn == inputs => SidebarRedraw::Nothing,
            Some(drawn) => {
                drawn.0.radar[0] = inputs.0.radar[0];
                // The quad repaints an outline that was and stays on it.
                if drawn.0.outline_on_minimap && inputs.0.outline_on_minimap {
                    drawn.0.outline = inputs.0.outline;
                }
                if *drawn == inputs {
                    SidebarRedraw::Minimap
                } else {
                    SidebarRedraw::Whole
                }
            }
            None => SidebarRedraw::Whole,
        };
        self.drawn = Some(inputs);
        redraw
    }

    /// The column's pixels under `quad`, as a scissor rect; None if it is off
    /// the surface.
    pub(crate) fn column_area(
        &self,
        quad: &crate::render::batch::SpriteInstance,
    ) -> Option<[u32; 4]> {
        let origin = self.column_camera.camera_pos;
        let scale = self.frame.draw_scale();
        let left = (quad.position[0] - origin[0]) * scale;
        let top = (quad.position[1] - origin[1]) * scale;
        let (width, height) = self.frame.surface_draw_size();
        // Float casts saturate, so a quad past the corner clamps to it.
        let right = ((left + quad.size[0] * scale).ceil() as u32).min(width);
        let bottom = ((top + quad.size[1] * scale).ceil() as u32).min(height);
        let (left, top) = (
            (left.floor() as u32).min(width),
            (top.floor() as u32).min(height),
        );
        (right > left && bottom > top).then_some([left, top, right - left, bottom - top])
    }

    /// Whether all of `quads` lie within the column pixels `area`.
    pub(crate) fn quads_within(
        &self,
        area: [u32; 4],
        quads: &[crate::render::batch::SpriteInstance],
    ) -> bool {
        quads.iter().all(|quad| {
            self.column_area(quad).is_some_and(|[x, y, width, height]| {
                x >= area[0]
                    && y >= area[1]
                    && x + width <= area[0] + area[2]
                    && y + height <= area[1] + area[3]
            })
        })
    }

    /// Place both copies in the match composition, at the window's pixels:
    /// each surface pixel covers [`SidebarFrame::window_copy_scale`]
    /// composition pixels. Without a match zoom that is the frame's copy.
    fn place(&self, queue: &wgpu::Queue) {
        let frame = self.frame;
        let (scale, sharp) = (frame.window_copy_scale(), frame.window_copy_is_sharp());
        let window = window_scale(frame);
        let at = |(x, y): (f32, f32)| (x * window, y * window);
        self.column
            .surface
            .place(queue, at(frame.side_origin()), scale, sharp);
        self.bar
            .surface
            .place(queue, at(frame.bar_origin()), scale, sharp);
    }

    /// Point each part's camera at its surface: the UI camera `ui`, sized to
    /// the surface in sidebar pixels, with its pixel (0, 0) at the part's
    /// corner in layout space. Write them after this frame's camera and Z
    /// origin. The bar's corner also shifts the camera row `vs_depth` reads;
    /// that moves all of the bar's layers alike, so their order stands.
    pub(crate) fn write_cameras(&mut self, queue: &wgpu::Queue, ui: CameraUniform) {
        let ui = sidebar_ui(ui);
        if self.written_ui == Some(ui) {
            return;
        }
        self.written_ui = Some(ui);
        let frame = self.frame;
        for (part, size, corner) in [
            (
                &self.column,
                frame.surface_size(),
                [frame.layout_left(), 0.0],
            ),
            (
                &self.bar,
                frame.bar_surface_size(),
                [frame.bar_layout_left(), frame.strip_layout_top()],
            ),
        ] {
            let uniform = CameraUniform {
                screen_size: [size.0 as f32, size.1 as f32],
                camera_pos: [ui.camera_pos[0] + corner[0], ui.camera_pos[1] + corner[1]],
                ..ui
            };
            queue.write_buffer(&part.camera_buffer, 0, bytemuck::bytes_of(&uniform));
        }
        let (width, height) = frame.surface_size();
        self.column_camera = CameraUniform {
            screen_size: [width as f32, height as f32],
            camera_pos: [ui.camera_pos[0] + frame.layout_left(), ui.camera_pos[1]],
            ..ui
        };
        // Text laid out for the surface's pixels lands there from the
        // origin; the Z row the depth reads stays the column's.
        let (width, height) = frame.surface_draw_size();
        let text = CameraUniform {
            screen_size: [width as f32, height as f32],
            camera_pos: [0.0; 2],
            native_z_origin_y: self.column_camera.camera_pos[1]
                + self.column_camera.native_z_origin_y,
            ..self.column_camera
        };
        queue.write_buffer(&self.text_camera_buffer, 0, bytemuck::bytes_of(&text));
    }

    /// The camera the column's text draws with in the surface:
    /// [`Self::camera`], or one at the surface's own pixels while they are
    /// finer than the sidebar's ([`column_text`]).
    pub(crate) fn text_camera(&self) -> &wgpu::BindGroup {
        if self.frame.draw_scale() > 1.0 {
            &self.text_camera
        } else {
            &self.column.camera
        }
    }

    pub(crate) fn prepare_packed(&mut self, device: &wgpu::Device, renderer: &TerrainDrawRenderer) {
        self.packed.get_or_insert_with(|| {
            renderer.create_packed_surface(
                device,
                self.column.surface.color_texture(),
                self.column.surface.depth_view(),
                self.frame.draw_scale(),
            )
        });
    }

    /// StripDraw6A9B46 follows the base cameo and precedes flash/clock/text.
    /// The destination operation has no Z admission or writes; the surface
    /// keeps its earlier layer depth for the resumed ordinary Load pass.
    pub(crate) fn draw_packed_layer(
        &self,
        renderer: &mut TerrainDrawRenderer,
        encoder: &mut wgpu::CommandEncoder,
        batch: &BatchRenderer,
        buffer: &wgpu::Buffer,
        instances: &[crate::render::batch::SpriteInstance],
        mask: &crate::render::batch::BatchTexture,
        kind: PackedSpriteKind,
    ) {
        let pieces: Vec<_> = instances
            .iter()
            .enumerate()
            .map(|(index, instance)| PackedCompositePiece {
                index: index as u32,
                instance,
                source: &mask.bind_group,
                material: batch.default_zshape_bind_group(),
                kind,
            })
            .collect();
        renderer.draw_packed_surface(
            encoder,
            self.column.surface.color_view(),
            self.column.surface.depth_view(),
            self.packed
                .as_ref()
                .expect("sidebar packed scratch prepared"),
            self.column_camera,
            &self.column.camera,
            buffer,
            &pieces,
        );
    }

    fn part(&self, part: SidebarPart) -> &PartSurface {
        match part {
            SidebarPart::Column => &self.column,
            SidebarPart::Bar => &self.bar,
        }
    }

    /// The camera bind group that draws into `part`'s surface.
    pub(crate) fn camera(&self, part: SidebarPart) -> &wgpu::BindGroup {
        &self.part(part).camera
    }

    /// Begin the pass that draws `part`: its whole surface cleared to `clear`,
    /// depth to empty.
    pub(crate) fn begin_pass<'e>(
        &self,
        part: SidebarPart,
        encoder: &'e mut wgpu::CommandEncoder,
        clear: wgpu::Color,
    ) -> wgpu::RenderPass<'e> {
        self.begin_surface_pass(
            part,
            encoder,
            wgpu::LoadOp::Clear(clear),
            wgpu::LoadOp::Clear(crate::render::native_z::STORED_DEPTH_CLEAR),
        )
    }

    pub(crate) fn begin_load_pass<'e>(
        &self,
        part: SidebarPart,
        encoder: &'e mut wgpu::CommandEncoder,
    ) -> wgpu::RenderPass<'e> {
        self.begin_surface_pass(part, encoder, wgpu::LoadOp::Load, wgpu::LoadOp::Load)
    }

    /// Begin the pass that redraws `area` of the column in place: the rest of
    /// its colour stays, and depth restarts for the layers drawn there.
    pub(crate) fn begin_area_pass<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
        [x, y, width, height]: [u32; 4],
    ) -> wgpu::RenderPass<'e> {
        let mut pass = self.begin_surface_pass(
            SidebarPart::Column,
            encoder,
            wgpu::LoadOp::Load,
            wgpu::LoadOp::Clear(crate::render::native_z::STORED_DEPTH_CLEAR),
        );
        pass.set_scissor_rect(x, y, width, height);
        pass
    }

    fn begin_surface_pass<'e>(
        &self,
        part: SidebarPart,
        encoder: &'e mut wgpu::CommandEncoder,
        color: wgpu::LoadOp<wgpu::Color>,
        depth: wgpu::LoadOp<f32>,
    ) -> wgpu::RenderPass<'e> {
        let surface = &self.part(part).surface;
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Sidebar Surface Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface.color_view(),
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: color,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: surface.depth_view(),
                depth_ops: Some(wgpu::Operations {
                    load: depth,
                    // Depth orders the part's own layers only.
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    /// Copy both surfaces into a match composition `pass`: the sidebar into
    /// its column, the bar into the strip left of it, each at the
    /// composition's pixels, `scale` per frame pixel, cut to `target`, the
    /// composition's size (`RendererState::frame_to_composition`). The
    /// scissor is left on the strip.
    pub(crate) fn composite(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        scale: f32,
        target: (u32, u32),
    ) {
        let rect = |rect| crate::app::renderer_state::scale_frame_rect(scale, target, rect);
        self.column
            .surface
            .draw(pass, None, rect(self.frame.column()));
        // A layout without a bar has no strip to copy into.
        if self.frame.has_bar() {
            self.bar.surface.draw(pass, None, rect(self.frame.strip()));
        }
    }
}

/// The UI camera `ui` for the sidebar's surfaces: they draw in sidebar
/// pixels at their own size, whatever the match zoom scales the frame's by.
fn sidebar_ui(ui: CameraUniform) -> CameraUniform {
    CameraUniform { zoom: 1.0, ..ui }
}

/// Window pixels per frame pixel: the match zoom `frame` was made for
/// (`SidebarFrame::in_window`), as its window copy scale over its frame copy
/// scale.
fn window_scale(frame: SidebarFrame) -> f32 {
    frame.window_copy_scale() / frame.copy_scale()
}

/// The column's text layers in their draw order: the queue counts
/// (`0x006A9D7B`), the Ready and Super labels (`0x006A9D94`), then the HOLD
/// label (`0x006AA014`) and the credits.
pub(crate) const TEXT_LAYERS: [&str; 3] =
    ["sidebar_queue_text", "sidebar_label_text", "sidebar_text"];

/// Where the sidebar column's text (its queue counts, labels and credits)
/// is drawn this frame, with the raster it is emitted for there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ColumnText {
    /// In the column's surface, between its other layers: at the surface's
    /// pixels, through the raster while they are finer than the sidebar's
    /// ([`SidebarSurface::text_camera`]).
    Surface(Option<TextRaster>),
    /// Over the column's copy in the composition (`dispatch_draw_passes`),
    /// in the frame's pixels with its UI camera.
    Frame(TextRaster),
    /// Over the column's copy in the window, after the composition went
    /// there (`window_ui::draw_battlefield`), in the window's pixels.
    Window(TextRaster),
}

impl ColumnText {
    /// The raster the text is emitted for; `None` is the surface's own.
    pub(crate) fn raster(self) -> Option<TextRaster> {
        match self {
            Self::Surface(raster) => raster,
            Self::Frame(raster) | Self::Window(raster) => Some(raster),
        }
    }
}

/// Where the column's text goes this frame. It is laid out in sidebar pixels
/// and drawn at the window's: in the surface, in gamemd's order among the
/// column's layers (`StripClass::Draw`, `0x006A9A3E..0x006AA014`), while
/// each surface pixel shows as one window pixel. Otherwise (the Large size,
/// or a zoom between whole steps) the column's copy would resample it, so
/// it goes over the copy: in the window after the zoom, or in the frame
/// without one or under the Surrender box, where the battlefield's text
/// stays too. Over the copy, the build clock (`0x006A9E97`) no longer blends
/// over the queue counts gamemd draws before it.
pub(crate) fn column_text(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> ColumnText {
    let input = &match_state.input;
    place_column_text(
        crate::app::state::sidebar_frame(
            platform,
            persistence,
            renderer,
            screen,
            &match_state.match_presentation.sidebar_layout_spec,
        ),
        [input.camera_x, input.camera_y],
        crate::app::presentation::window_ui::battlefield_raster(renderer, screen, match_state),
    )
}

/// [`column_text`] for `frame`, the UI camera at `camera`, and `window`, the
/// battlefield's window raster this frame.
fn place_column_text(
    frame: SidebarFrame,
    [camera_x, camera_y]: [f32; 2],
    window: Option<TextRaster>,
) -> ColumnText {
    let camera = [camera_x + frame.layout_left(), camera_y];
    if frame.draws_window_pixels() {
        let scale = frame.draw_scale();
        return ColumnText::Surface((scale > 1.0).then_some(TextRaster {
            scale,
            origin: [0.0; 2],
            camera,
        }));
    }
    match window {
        Some(window) => ColumnText::Window(TextRaster {
            scale: frame.scale() * window.scale,
            origin: [
                window.origin[0] + frame.frame_left() * window.scale,
                window.origin[1] + frame.frame_top() * window.scale,
            ],
            camera,
        }),
        None => ColumnText::Frame(TextRaster {
            scale: frame.scale(),
            origin: [camera_x + frame.frame_left(), camera_y + frame.frame_top()],
            camera,
        }),
    }
}

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;
    use crate::render::gpu::block_on;
    use std::time::Duration;
    use wgpu::util::DeviceExt;

    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
    }

    fn gpu() -> Gpu {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("explicit sidebar surface GPU test requires adapter");
        let (device, queue) =
            block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        Gpu { device, queue }
    }

    fn extent((width, height): (u32, u32)) -> wgpu::Extent3d {
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        }
    }

    /// Write `texel(x, y)` into every pixel of `part`'s surface.
    fn fill(
        gpu: &Gpu,
        surface: &SidebarSurface,
        part: SidebarPart,
        texel: impl Fn(u32, u32) -> [u8; 4],
    ) {
        let (width, height) = match part {
            SidebarPart::Column => surface.frame.surface_draw_size(),
            SidebarPart::Bar => surface.frame.bar_draw_size(),
        };
        let texels: Vec<u8> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .flat_map(|(x, y)| texel(x, y))
            .collect();
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: surface.part(part).surface.color_texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            extent((width, height)),
        );
    }

    /// Run `draw`, then composite the surfaces into a match composition pass
    /// with a depth attachment, as the match does, and read it back as BGRA:
    /// the `frame_size` frame at the window's pixels its zoom gives.
    fn composite(
        gpu: &Gpu,
        surface: &SidebarSurface,
        format: wgpu::TextureFormat,
        frame_size: (u32, u32),
        draw: impl FnOnce(&mut wgpu::CommandEncoder),
    ) -> Vec<u8> {
        let scale = window_scale(surface.frame);
        let frame_size = (
            (frame_size.0 as f32 * scale).round() as u32,
            (frame_size.1 as f32 * scale).round() as u32,
        );
        let target = |format: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: extent(frame_size),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | usage,
                view_formats: &[],
            })
        };
        let color = target(format, wgpu::TextureUsages::COPY_SRC);
        let depth = target(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::empty(),
        );
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        draw(&mut encoder);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 9.0 / 255.0,
                            g: 9.0 / 255.0,
                            b: 9.0 / 255.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            surface.composite(&mut pass, scale, frame_size);
        }
        let readback = crate::render::frame_readback::PendingTextureReadback::encode(
            &gpu.device,
            &mut encoder,
            &color,
            format,
            frame_size.0,
            frame_size.1,
        )
        .unwrap();
        let submission = gpu.queue.submit([encoder.finish()]);
        readback
            .finish(&gpu.device, submission, Duration::from_secs(20))
            .unwrap()
    }

    fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * width + x) * 4) as usize;
        pixels[at..at + 4].try_into().unwrap()
    }

    /// The top-left composition pixel of frame pixel `(x, y)` in a
    /// [`composite`] of a `width`-wide frame at `scale`, a whole zoom.
    fn frame_pixel(pixels: &[u8], width: u32, scale: f32, x: u32, y: u32) -> [u8; 4] {
        let scale = scale as u32;
        pixel(pixels, width * scale, x * scale, y * scale)
    }

    /// The column's text draws in the surface where the surface pixels are the
    /// shown pixels (Large's 1:1 copy, the whole steps), and over the copy
    /// where it would be resampled.
    #[test]
    fn column_text_goes_where_its_pixels_are_shown() {
        let camera = [64.0, 32.0];
        // Large unzoomed at 1080p copies 1:1, so the surface holds the text
        // at its 1.5x pixels.
        let large = SidebarFrame::new(1920, 1080, 150).in_window(100, true);
        let ColumnText::Surface(Some(raster)) = place_column_text(large, camera, None) else {
            panic!("Large's 1:1 copy keeps its text in the surface");
        };
        assert_eq!(raster.scale, 1.5);
        let corner = [camera[0] + large.layout_left(), camera[1]];
        assert_eq!(raster.point(corner), [0.0, 0.0]);
        assert_eq!(raster.point([corner[0] + 10.0, 36.0]), [15.0, 6.0]);
        // Normal zoomed 150%: over the window copy, where the zoom puts it.
        let window = TextRaster {
            scale: 1.5,
            origin: [10.0, 0.0],
            camera,
        };
        let normal = SidebarFrame::new(1280, 720, 100).in_window(150, false);
        let ColumnText::Window(raster) = place_column_text(normal, camera, Some(window)) else {
            panic!("a 150% zoom keeps the text in the surface");
        };
        let corner = [camera[0] + normal.layout_left(), camera[1] + 2.0];
        assert_eq!(raster.point(corner), [10.0 + 1112.0 * 1.5, 3.0]);
        // Whole steps: XL unzoomed draws at the surface's 2x pixels, Normal
        // at its own.
        let xl = SidebarFrame::new(1920, 1080, 200).in_window(100, false);
        let surface = TextRaster {
            scale: 2.0,
            origin: [0.0; 2],
            camera: [64.0 + 792.0, 32.0],
        };
        assert_eq!(
            place_column_text(xl, camera, None),
            ColumnText::Surface(Some(surface))
        );
        let native = SidebarFrame::native(800, 600);
        assert_eq!(
            place_column_text(native, camera, Some(window)),
            ColumnText::Surface(None)
        );
    }

    /// At Normal the copy is 1:1 in the production sRGB format: every byte
    /// comes through unchanged, so Normal frames are the ones drawn before
    /// the surfaces existed.
    #[test]
    #[ignore = "requires a wgpu adapter; executes the production scaled copy"]
    fn normal_copies_every_byte_unchanged() {
        const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
        let gpu = gpu();
        let frame = SidebarFrame::native(400, 300);
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, SRGB);
        let surface = SidebarSurface::new(&gpu.device, &gpu.queue, SRGB, &batch, frame);
        // Each channel runs through all 256 values across the column.
        let texel = |x: u32, y: u32| {
            let i = x + y * 168;
            [i as u8, (i * 7 + 3) as u8, (255 - i % 256) as u8, 255]
        };
        fill(&gpu, &surface, SidebarPart::Column, texel);
        fill(&gpu, &surface, SidebarPart::Bar, |x, y| texel(x % 168, y));
        let pixels = composite(&gpu, &surface, SRGB, (400, 300), |_| {});
        for y in 0..300 {
            for x in 0..168 {
                assert_eq!(pixel(&pixels, 400, 232 + x, y), texel(x, y), "({x}, {y})");
            }
        }
        for y in 0..32 {
            for x in 0..232 {
                assert_eq!(pixel(&pixels, 400, x, 268 + y), texel(x % 168, y));
            }
        }
    }

    /// Zoomed to 300%, the surfaces draw 3x3 window pixels per sidebar
    /// pixel, and the copy puts every one of them in the match composition,
    /// which is at the window's pixels.
    #[test]
    #[ignore = "requires a wgpu adapter; executes the production sidebar copies"]
    fn zoomed_copy_shows_every_surface_pixel() {
        const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
        let gpu = gpu();
        let frame = SidebarFrame::native(400, 300).in_window(300, false);
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, SRGB);
        let surface = SidebarSurface::new(&gpu.device, &gpu.queue, SRGB, &batch, frame);
        // The column and the bar differ at every surface pixel.
        let column = |x: u32, y: u32| [x as u8, (y * 7) as u8, (x + y) as u8 | 1, 255];
        let bar = |x: u32, y: u32| [(x / 3) as u8, (y * 5) as u8, (x + 2 * y) as u8 | 1, 255];
        fill(&gpu, &surface, SidebarPart::Column, column);
        fill(&gpu, &surface, SidebarPart::Bar, bar);
        let pixels = composite(&gpu, &surface, SRGB, (400, 300), |_| {});
        for y in 0..900 {
            for x in 0..504 {
                assert_eq!(pixel(&pixels, 1200, 696 + x, y), column(x, y), "({x}, {y})");
            }
        }
        for y in 0..96 {
            for x in 0..696 {
                assert_eq!(
                    pixel(&pixels, 1200, x, 804 + y),
                    bar(x, y),
                    "bar ({x}, {y})"
                );
            }
        }
    }

    #[test]
    #[ignore = "requires a wgpu adapter and configured retail assets; executes native sidebar packed drawing"]
    fn darken_indices_clip_native_size_and_load_later_status_layers() {
        use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
        use crate::assets::pal_file::Palette;
        use crate::render::batch::SpriteInstance;
        use crate::render::sidebar_chrome::SidebarDarkenMask;
        use crate::render::terrain_draw_gpu_tests::encoded;
        use crate::ui::sidebar::Rect;
        const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
        let gpu = gpu();
        let path = std::env::var_os("RA2_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                crate::util::config::GameConfig::load()
                    .ok()
                    .map(|config| config.paths.ra2_dir)
            })
            .expect("retail data is required for the explicit sidebar GPU test");
        let assets = AssetManager::new(&path, MediaArchiveMode::STOCK_DIGITAL).unwrap();
        // Exercise the original global resolver and production frame decoder.
        let source = assets
            .load_file_from_mix("DARKEN.SHP")
            .expect("retail DARKEN.SHP");
        assert_eq!(
            crate::util::sha256::sha256_hex(&source.bytes),
            "c4455fb08203e0089af30dab37553854a892ba73eb3e00bb0fdf049576cf4e8a"
        );
        let palette = Palette::from_rgb(|_| [255, 37, 91]);
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, FORMAT);
        let retail =
            SidebarDarkenMask::from_bytes(&gpu.device, &gpu.queue, &batch, &source.bytes, &palette)
                .unwrap();
        assert_eq!(
            (
                retail.texture.width,
                retail.texture.height,
                retail.frame_origin
            ),
            (75, 52, [0.0; 2])
        );
        let custom_indices: Vec<u8> = (0..75 * 52)
            .map(|i| if i % 7 == 0 { 0 } else { 3 })
            .collect();
        let custom = SidebarDarkenMask {
            texture: batch.create_texture_on_device(
                &gpu.device,
                &gpu.queue,
                &vec![255; 75 * 52 * 4],
                75,
                52,
                Some(&custom_indices),
            ),
            frame_origin: [3.0, 4.0],
        };
        let native_half =
            crate::test_fixture::bytes("tools/terrain_draw_oracle/fixtures/half-rgb565.bin");
        let at = usize::from(u16::MAX) * 2;
        let half_white = encoded(
            u16::from_le_bytes([native_half[at], native_half[at + 1]]),
            FORMAT,
        );
        let frame = SidebarFrame::native(400, 300);
        let mut world = batch.camera_uniform();
        world.camera_pos = [123.25, 456.75];
        world.zoom = 1.5;
        batch.write_camera(&gpu.queue, world);
        let slot = Rect {
            x: frame.layout_left() + 24.0,
            y: 100.0,
            w: 60.0,
            h: 48.0,
        };
        let solid =
            batch.create_texture_on_device(&gpu.device, &gpu.queue, &[255, 0, 0, 255], 1, 1, None);
        let hold_solid =
            batch.create_texture_on_device(&gpu.device, &gpu.queue, &[0, 0, 255, 255], 1, 1, None);
        let clock_texture = batch.create_texture_on_device(
            &gpu.device,
            &gpu.queue,
            &[0, 0, 0, 255],
            1,
            1,
            Some(&[1]),
        );
        let clock_frames = vec![
            crate::render::sidebar_chrome::SidebarChromeEntry {
                uv_origin: [0.0; 2],
                uv_size: [1.0; 2],
                pixel_size: [60.0, 48.0],
            };
            55
        ];
        // Zoomed to 300%, each piece covers its pixels' 3x3 blocks, which the
        // copy shows at the window's pixels.
        let zoomed = frame.in_window(300, false);
        for (frame, mask, synthetic) in [
            (frame, &retail, false),
            (frame, &custom, true),
            (zoomed, &retail, false),
            (zoomed, &custom, true),
        ] {
            let mut renderer = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, FORMAT, &batch);
            let mut surface = SidebarSurface::new(&gpu.device, &gpu.queue, FORMAT, &batch, frame);
            surface.write_cameras(&gpu.queue, batch.ui_camera_uniform());
            surface.prepare_packed(&gpu.device, &renderer);
            let instance = crate::app::presentation::sidebar_build::build_darken_instance(
                mask,
                slot,
                world.camera_pos,
                1,
            )
            .unwrap();
            assert_eq!(
                instance.size,
                if synthetic {
                    [57.0, 44.0]
                } else {
                    [60.0, 48.0]
                }
            );
            let buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::bytes_of(&instance),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let status = SpriteInstance {
                position: [
                    slot.x + world.camera_pos[0] + 10.0,
                    slot.y + world.camera_pos[1] + 10.0,
                ],
                size: [2.0; 2],
                uv_size: [1.0; 2],
                tint: [1.0; 3],
                alpha: 1.0,
                depth: 0.00042,
                ..Default::default()
            };
            let status_buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::bytes_of(&status),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let clock = crate::app::presentation::sidebar_build::build_gclock_instance(
                &clock_frames,
                53.0 / 54.0,
                slot,
                world.camera_pos,
            )
            .unwrap();
            let clock_buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::bytes_of(&clock),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let hold = SpriteInstance {
                position: [
                    slot.x + world.camera_pos[0] + 20.0,
                    slot.y + world.camera_pos[1] + 20.0,
                ],
                ..status
            };
            let hold_buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::bytes_of(&hold),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let pixels = composite(&gpu, &surface, FORMAT, (400, 300), |encoder| {
                drop(surface.begin_pass(SidebarPart::Column, encoder, wgpu::Color::WHITE));
                surface.draw_packed_layer(
                    &mut renderer,
                    encoder,
                    &batch,
                    &buffer,
                    &[instance],
                    &mask.texture,
                    PackedSpriteKind::IndexMask,
                );
                let mut pass = surface.begin_load_pass(SidebarPart::Column, encoder);
                pass.set_pipeline(batch.overlay_pipeline());
                pass.set_bind_group(0, surface.camera(SidebarPart::Column), &[]);
                pass.set_bind_group(1, &solid.bind_group, &[]);
                pass.set_vertex_buffer(0, status_buffer.slice(..));
                pass.draw(0..6, 0..1);
                drop(pass);
                // The original404 reads the earlier badge glyph destination.
                surface.draw_packed_layer(
                    &mut renderer,
                    encoder,
                    &batch,
                    &clock_buffer,
                    &[clock],
                    &clock_texture,
                    PackedSpriteKind::UiPalette,
                );
                let mut pass = surface.begin_load_pass(SidebarPart::Column, encoder);
                pass.set_pipeline(batch.overlay_pipeline());
                pass.set_bind_group(0, surface.camera(SidebarPart::Column), &[]);
                pass.set_bind_group(1, &hold_solid.bind_group, &[]);
                pass.set_vertex_buffer(0, hold_buffer.slice(..));
                pass.draw(0..6, 0..1);
                drop(pass);
                drop(surface.begin_pass(SidebarPart::Bar, encoder, wgpu::Color::BLACK));
            });
            for y in 98..154 {
                for x in 22..102 {
                    let local = [x as i32 - 24, y as i32 - 100];
                    let inside = local[0] >= 0 && local[0] < 60 && local[1] >= 0 && local[1] < 48;
                    let admitted = inside
                        && if synthetic {
                            let sx = local[0] - 3;
                            let sy = local[1] - 4;
                            sx >= 0 && sy >= 0 && custom_indices[(sy * 75 + sx) as usize] != 0
                        } else {
                            true
                        };
                    let half_word = |word: u16| {
                        let at = usize::from(word) * 2;
                        u16::from_le_bytes([native_half[at], native_half[at + 1]])
                    };
                    let expected = if (20..22).contains(&local[0]) && (20..22).contains(&local[1]) {
                        [255, 0, 0, 255] // later HOLD/status stays above clock
                    } else if (10..12).contains(&local[0]) && (10..12).contains(&local[1]) {
                        encoded(half_word(0xf800), FORMAT) // earlier badge was read by404
                    } else if admitted {
                        encoded(half_word(half_word(u16::MAX)), FORMAT)
                    } else if inside {
                        half_white
                    } else {
                        [255; 4]
                    };
                    assert_eq!(
                        frame_pixel(
                            &pixels,
                            400,
                            window_scale(frame),
                            frame.frame_left() as u32 + x,
                            y
                        ),
                        expected,
                        "scale={} synthetic={synthetic} ({x},{y})",
                        frame.draw_scale()
                    );
                }
            }
            // Scratch survives same-size frame changes; a real resize retires
            // it with its source attachments instead of switching tactical data.
            surface.set_frame(&gpu.device, &gpu.queue, frame);
            assert!(surface.packed.is_some());
            surface.set_frame(&gpu.device, &gpu.queue, SidebarFrame::native(400, 301));
            assert!(surface.packed.is_none());
            surface.prepare_packed(&gpu.device, &renderer);
            assert!(surface.packed.is_some());
        }
    }

    /// At Large the column surface holds 1.5 pixels per sidebar pixel, so a
    /// packed mask must cover the 1.5x pixels its cell shows, through the 1:1
    /// copy. A whole-scale packed rectangle would leave the mask's right and
    /// bottom thirds unmasked.
    #[test]
    #[ignore = "requires a wgpu adapter; executes the production sidebar packed drawing at Large"]
    fn a_large_packed_mask_covers_its_shown_cell() {
        use crate::app::presentation::sidebar_build::build_darken_instance;
        use crate::render::sidebar_chrome::SidebarDarkenMask;
        use crate::ui::sidebar::Rect;
        const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
        let gpu = gpu();
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, FORMAT);
        let frame = SidebarFrame::new(400, 300, 150).in_window(100, false);
        assert_eq!(frame.surface_draw_size(), (252, 300));
        let mut renderer = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, FORMAT, &batch);
        let mut surface = SidebarSurface::new(&gpu.device, &gpu.queue, FORMAT, &batch, frame);
        surface.write_cameras(&gpu.queue, batch.ui_camera_uniform());
        surface.prepare_packed(&gpu.device, &renderer);
        let indices = vec![3u8; 60 * 48];
        let mask = SidebarDarkenMask {
            texture: batch.create_texture_on_device(
                &gpu.device,
                &gpu.queue,
                &vec![255; 60 * 48 * 4],
                60,
                48,
                Some(&indices),
            ),
            frame_origin: [0.0; 2],
        };
        let slot = Rect {
            x: frame.layout_left() + 24.0,
            y: 100.0,
            w: 60.0,
            h: 48.0,
        };
        let instance = build_darken_instance(&mask, slot, [0.0; 2], 1).unwrap();
        let buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::bytes_of(&instance),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let pixels = composite(&gpu, &surface, FORMAT, (400, 300), |encoder| {
            drop(surface.begin_pass(SidebarPart::Column, encoder, wgpu::Color::WHITE));
            surface.draw_packed_layer(
                &mut renderer,
                encoder,
                &batch,
                &buffer,
                &[instance],
                &mask.texture,
                PackedSpriteKind::IndexMask,
            );
        });
        // The cell (24, 100)-(84, 148) in sidebar pixels covers
        // (36, 150)-(126, 222) on the 1.5x surface, copied 1:1 at the column.
        let column_x = frame.column().0;
        let white = [255u8; 4];
        assert_ne!(
            pixel(&pixels, 400, column_x + 36 + 45, 150 + 36),
            white,
            "the cell's middle is masked"
        );
        for (x, y) in [
            (column_x + 35, 150 + 36),
            (column_x + 36 + 90, 150 + 36),
            (column_x + 36 + 45, 149),
            (column_x + 36 + 45, 150 + 72),
        ] {
            assert_eq!(pixel(&pixels, 400, x, y), white, "({x}, {y}) is outside");
        }
    }

    /// Only the minimap's composition changing redraws only its quad, in the
    /// surface's own pixels.
    #[test]
    #[ignore = "requires a wgpu adapter; makes the production sidebar surface"]
    fn a_minimap_composition_change_redraws_only_its_quad() {
        const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
        let gpu = gpu();
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, SRGB);
        let frame = SidebarFrame::native(400, 300);
        let mut surface = SidebarSurface::new(&gpu.device, &gpu.queue, SRGB, &batch, frame);
        let picture = |stream, minimap, housing, outline, outline_on_minimap| SidebarPicture {
            streams: vec![stream],
            textures: Vec::new(),
            radar: [minimap, housing],
            outline,
            outline_on_minimap,
            text_in_surface: true,
        };
        surface.write_cameras(&gpu.queue, batch.ui_camera_uniform());
        for (stream, minimap, housing, outline, on_minimap, redraw) in [
            (1, 1, 1, 1, true, SidebarRedraw::Whole),
            (1, 1, 1, 1, true, SidebarRedraw::Nothing),
            (1, 2, 1, 1, true, SidebarRedraw::Minimap),
            (1, 2, 1, 2, true, SidebarRedraw::Minimap),
            (1, 2, 1, 3, false, SidebarRedraw::Whole),
            (1, 2, 1, 4, false, SidebarRedraw::Whole),
            (1, 2, 2, 4, false, SidebarRedraw::Whole),
            (2, 3, 2, 4, false, SidebarRedraw::Whole),
        ] {
            assert_eq!(
                surface.redraw_for(picture(stream, minimap, housing, outline, on_minimap)),
                redraw
            );
        }
        // The camera's own scroll moves nothing on the surface.
        let scrolled = CameraUniform {
            camera_pos: [50.0, 25.0],
            ..batch.ui_camera_uniform()
        };
        surface.write_cameras(&gpu.queue, scrolled);
        assert_eq!(
            surface.redraw_for(picture(2, 3, 2, 4, false)),
            SidebarRedraw::Nothing
        );

        let origin = surface.column_camera.camera_pos;
        let quad = |x, y| crate::render::batch::SpriteInstance {
            position: [origin[0] + x, origin[1] + y],
            size: [140.0, 108.0],
            ..Default::default()
        };
        assert_eq!(
            surface.column_area(&quad(16.0, 49.0)),
            Some([16, 49, 140, 108])
        );
        assert_eq!(
            surface.column_area(&quad(16.0, 250.0)),
            Some([16, 250, 140, 50])
        );
        assert_eq!(surface.column_area(&quad(16.0, 300.0)), None);
        surface.set_frame(&gpu.device, &gpu.queue, frame.in_window(300, false));
        assert_eq!(
            surface.column_area(&quad(16.0, 49.0)),
            Some([48, 147, 420, 324])
        );
    }
}
