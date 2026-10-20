//! The sidebar's own surfaces: gamemd's `g_SidebarSurface`, and the command
//! bar along the frame's bottom.
//!
//! Both draw in their own native pixels (`ui::sidebar::frame`): the sidebar
//! into a surface 168 pixels wide, the bar into one 32 pixels tall.
//! `SidebarClass::BlitToScreen` (`6A70E0`) puts the finished sidebar in the
//! frame's right-hand column; here that copy, and the bar's into the bottom
//! strip, also scale by the player's Sidebar size, sharp at the whole sizes.
//! At Normal they are 1:1.
//!
//! Drawing into the surfaces also keeps the sidebar's depth there: the frame's
//! column keeps the cleared depth, so the tooltips and the software cursor
//! drawn after the copy always show over the radar, as gamemd draws the mouse
//! last.

use crate::render::batch::{BatchRenderer, CameraUniform};
use crate::render::scaled_copy::ScaledSurface;
use crate::render::terrain_draw::{
    PackedCompositePiece, PackedSpriteKind, PackedSurface, TerrainDrawRenderer,
};
use crate::ui::sidebar::SidebarFrame;

/// A part of the sidebar with a surface of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarPart {
    /// The right-hand column.
    Column,
    /// The command bar on the bottom strip.
    Bar,
}

pub(crate) struct SidebarSurface {
    column: PartSurface,
    bar: PartSurface,
    frame: SidebarFrame,
    packed: Option<PackedSurface>,
    column_camera: CameraUniform,
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
        let surface = Self {
            column: part("Sidebar Surface", frame.surface_size()),
            bar: part("Command Bar Surface", frame.bar_surface_size()),
            frame,
            packed: None,
            column_camera: batch.ui_camera_uniform(),
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
        if frame.surface_size() != self.frame.surface_size() {
            self.packed = None;
        }
        self.column.surface.resize(device, frame.surface_size());
        self.bar.surface.resize(device, frame.bar_surface_size());
        self.frame = frame;
        self.place(queue);
    }

    fn place(&self, queue: &wgpu::Queue) {
        let frame = self.frame;
        let (scale, sharp) = (frame.scale(), frame.is_sharp());
        self.column
            .surface
            .place(queue, (frame.frame_left(), 0.0), scale, sharp);
        self.bar
            .surface
            .place(queue, frame.bar_origin(), scale, sharp);
    }

    /// Point each part's camera at its surface: the UI camera `ui`, sized to
    /// the surface, with its pixel (0, 0) at the part's corner in layout
    /// space. Write them after this frame's camera and Z origin. The bar's
    /// corner also shifts the camera row `vs_depth` reads; that moves all of
    /// the bar's layers alike, so their order stands.
    pub(crate) fn write_cameras(&mut self, queue: &wgpu::Queue, ui: CameraUniform) {
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
                [0.0, frame.strip_layout_top()],
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
    }

    pub(crate) fn prepare_packed(&mut self, device: &wgpu::Device, renderer: &TerrainDrawRenderer) {
        self.packed.get_or_insert_with(|| {
            renderer.create_packed_surface(
                device,
                self.column.surface.color_texture(),
                self.column.surface.depth_view(),
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
        self.begin_surface_pass(part, encoder, Some(clear))
    }

    pub(crate) fn begin_load_pass<'e>(
        &self,
        part: SidebarPart,
        encoder: &'e mut wgpu::CommandEncoder,
    ) -> wgpu::RenderPass<'e> {
        self.begin_surface_pass(part, encoder, None)
    }

    fn begin_surface_pass<'e>(
        &self,
        part: SidebarPart,
        encoder: &'e mut wgpu::CommandEncoder,
        clear: Option<wgpu::Color>,
    ) -> wgpu::RenderPass<'e> {
        let surface = &self.part(part).surface;
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Sidebar Surface Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface.color_view(),
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: surface.depth_view(),
                depth_ops: Some(wgpu::Operations {
                    load: clear.map_or(wgpu::LoadOp::Load, |_| {
                        wgpu::LoadOp::Clear(crate::render::native_z::STORED_DEPTH_CLEAR)
                    }),
                    // Depth orders the part's own layers only.
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    /// Copy both surfaces into a frame `pass`: the sidebar into its column,
    /// the bar into the strip left of it. The scissor is left on the strip.
    pub(crate) fn composite(&self, pass: &mut wgpu::RenderPass<'_>) {
        let (x, y, width, height) = self.frame.column();
        self.column.surface.draw(pass, None, [x, y, width, height]);
        let (x, y, width, height) = self.frame.strip();
        self.bar.surface.draw(pass, None, [x, y, width, height]);
    }
}

#[cfg(test)]
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
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
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
            SidebarPart::Column => surface.frame.surface_size(),
            SidebarPart::Bar => surface.frame.bar_surface_size(),
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

    /// Run `draw`, then composite the surfaces into a frame pass with a depth
    /// attachment, as the match does, and read the frame back as BGRA.
    fn composite(
        gpu: &Gpu,
        surface: &SidebarSurface,
        format: wgpu::TextureFormat,
        frame_size: (u32, u32),
        draw: impl FnOnce(&mut wgpu::CommandEncoder),
    ) -> Vec<u8> {
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
            });
            surface.composite(&mut pass);
        }
        let readback = crate::render::frame_readback::PendingBgra8Readback::encode(
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
        for (mask, synthetic) in [(&retail, false), (&custom, true)] {
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
                        pixel(&pixels, 400, frame.frame_left() as u32 + x, y),
                        expected,
                        "synthetic={synthetic} ({x},{y})"
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
}
