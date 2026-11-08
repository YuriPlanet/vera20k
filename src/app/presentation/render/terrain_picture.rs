//! The finished terrain layer, kept between frames.
//!
//! VERA scheduling only; no native counterpart. Terrain tiles, smudges and
//! cell bodies are the frame's widest draws, and their pixels are a pure
//! function of [`TerrainInputs`]. While those hold still, the layer's colour
//! and depth come back by copy instead of being drawn again. The copy out
//! waits for the second equal frame, so a scrolling camera pays nothing.

use crate::app::presentation::instances::CellBodyDraw;
use crate::render::batch::CameraUniform;

/// Every instance stream the terrain layer draws.
pub(super) const TERRAIN_STREAMS: [&str; 3] = ["terrain", "smudge", "cell_body"];

const COPIES: wgpu::TextureUsages =
    wgpu::TextureUsages::COPY_SRC.union(wgpu::TextureUsages::COPY_DST);

/// What the terrain layer's pixels are drawn from.
#[derive(PartialEq)]
pub(super) struct TerrainInputs {
    /// The generation of each of [`TERRAIN_STREAMS`].
    pub(super) streams: [u64; 3],
    pub(super) cell_bodies: Vec<CellBodyDraw>,
    pub(super) camera: CameraUniform,
    /// Group 0 of the world pipelines, and the generation of the A plane it
    /// samples.
    pub(super) camera_group: wgpu::BindGroup,
    pub(super) a_plane: u64,
    pub(super) tactical: [u32; 4],
    pub(super) atlases: [Option<wgpu::BindGroup>; 4],
    /// The colour and depth targets it is drawn into.
    pub(super) targets: [wgpu::Texture; 2],
}

/// How a frame gets its terrain layer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TerrainStep {
    Draw,
    /// Draw it and keep a copy: the inputs held still since the last frame.
    DrawAndKeep,
    /// [`TerrainPicture::restore`] the kept copy.
    Restore,
}

#[derive(Default)]
pub(crate) struct TerrainPicture {
    /// The inputs of the last frame's layer.
    drawn: Option<TerrainInputs>,
    /// Textures for the kept colour and depth.
    kept: Option<[wgpu::Texture; 2]>,
    /// Whether `kept` holds the layer `drawn` describes.
    valid: bool,
}

impl TerrainPicture {
    /// How this frame gets its layer. The WebGL fallback and a target that
    /// cannot be copied always draw it.
    pub(super) fn step(&mut self, device: &wgpu::Device, inputs: TerrainInputs) -> TerrainStep {
        let copyable = !crate::render::gpu::webgl_fallback(device)
            && inputs
                .targets
                .iter()
                .all(|target| target.usage().contains(COPIES));
        if !copyable || self.drawn.as_ref() != Some(&inputs) {
            self.valid = false;
            self.drawn = copyable.then_some(inputs);
            return TerrainStep::Draw;
        }
        if self.valid {
            TerrainStep::Restore
        } else {
            TerrainStep::DrawAndKeep
        }
    }

    /// Copy the layer just drawn out of its targets. No pass may be open.
    pub(super) fn keep(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) {
        let targets = &self
            .drawn
            .as_ref()
            .expect("the layer to keep was just stepped")
            .targets;
        let fits = |kept: &wgpu::Texture, target: &wgpu::Texture| {
            kept.size() == target.size() && kept.format() == target.format()
        };
        if !self
            .kept
            .as_ref()
            .is_some_and(|kept| fits(&kept[0], &targets[0]) && fits(&kept[1], &targets[1]))
        {
            self.kept = Some(targets.each_ref().map(|target| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Kept terrain layer"),
                    size: target.size(),
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: target.format(),
                    usage: COPIES,
                    view_formats: &[],
                })
            }));
        }
        let kept = self.kept.as_ref().expect("kept textures were just ensured");
        for (target, kept) in targets.iter().zip(kept) {
            encoder.copy_texture_to_texture(
                target.as_image_copy(),
                kept.as_image_copy(),
                target.size(),
            );
        }
        self.valid = true;
    }

    /// Copy the kept layer into its targets. No pass may be open.
    pub(super) fn restore(&self, encoder: &mut wgpu::CommandEncoder) {
        let targets = &self
            .drawn
            .as_ref()
            .expect("a kept layer has inputs")
            .targets;
        let kept = self.kept.as_ref().expect("a kept layer has textures");
        for (target, kept) in targets.iter().zip(kept) {
            encoder.copy_texture_to_texture(
                kept.as_image_copy(),
                target.as_image_copy(),
                target.size(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::gpu::block_on;
    use bytemuck::Zeroable;

    #[test]
    #[ignore = "requires a wgpu adapter; executes the production terrain copies"]
    fn the_second_equal_frame_keeps_the_layer_and_the_third_restores_it() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("explicit terrain picture GPU test requires adapter");
        let (device, queue) =
            block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | COPIES,
            view_formats: &[],
        });
        let (depth, depth_view) =
            crate::render::scaled_copy::create_depth_target(&device, "Test", (4, 4));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[],
        });
        let inputs = |stream| TerrainInputs {
            streams: [stream, 0, 0],
            cell_bodies: Vec::new(),
            camera: CameraUniform::zeroed(),
            camera_group: group.clone(),
            a_plane: 0,
            tactical: [0, 0, 4, 4],
            atlases: [None, None, None, None],
            targets: [color.clone(), depth.clone()],
        };
        let color_view = color.create_view(&Default::default());
        let clear = |encoder: &mut wgpu::CommandEncoder, g: f64, z: f32| {
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(z),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }));
        };

        let mut picture = TerrainPicture::default();
        assert!(picture.step(&device, inputs(1)) == TerrainStep::Draw);
        assert!(picture.step(&device, inputs(1)) == TerrainStep::DrawAndKeep);
        let mut encoder = device.create_command_encoder(&Default::default());
        clear(&mut encoder, 1.0, 0.25);
        picture.keep(&device, &mut encoder);
        clear(&mut encoder, 0.0, 0.75);
        assert!(picture.step(&device, inputs(1)) == TerrainStep::Restore);
        picture.restore(&mut encoder);

        let bytes = 256 * 4;
        let readback = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        };
        let (color_read, depth_read) = (readback("Colour"), readback("Depth"));
        let layout = wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(256),
            rows_per_image: None,
        };
        let size = color.size();
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &color_read,
                layout,
            },
            size,
        );
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                aspect: wgpu::TextureAspect::DepthOnly,
                ..depth.as_image_copy()
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &depth_read,
                layout,
            },
            size,
        );
        queue.submit([encoder.finish()]);
        for buffer in [&color_read, &depth_read] {
            buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        }
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert_eq!(
            color_read.slice(..4).get_mapped_range().unwrap()[..],
            [0, 255, 0, 255]
        );
        assert_eq!(
            depth_read.slice(..4).get_mapped_range().unwrap()[..],
            0.25_f32.to_ne_bytes()
        );

        assert!(picture.step(&device, inputs(2)) == TerrainStep::Draw);
    }
}
