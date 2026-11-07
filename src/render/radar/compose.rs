//! The radar's pictures, composed on the GPU (`RadarClass::Update @
//! 0x00656EC0`): the generated terrain under each pixel's shroud and fog
//! (`RenderCellPixel @ 0x00655C50`), then the object, radar-event and beacon
//! stores in update order, presented into the minimap's texture and, when it
//! is on, VERA's HD radar ([`super::hd`]).
//!
//! The CPU keeps what decides those pixels: the generated terrain and its x87
//! sampler ([`super::terrain`]), the cell each pixel reads (the native
//! inverse, uploaded once per geometry), the viewer's cell flags and the
//! ordered stores. The mapless fallback, which has no generated surface,
//! composes its terrain on the CPU and uploads that as the composite's base.

use bytemuck::{Pod, Zeroable};

use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::shroud_buffer::{InstanceUpload, texture_entry, uniform_entry};

use super::geometry::NativeRadarSurfaceGeometry;
use super::hd::HdRadar;
use super::helpers::{MINIMAP_HEIGHT, MINIMAP_WIDTH};
use super::terrain::NativeRadarTerrainSurface;

/// Each composite texel is one aperture pixel's RGBA bytes, little-endian.
const COMPOSITE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
/// The pictures the batch pipelines sample.
pub(super) const PICTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// One store over the composed terrain: an aperture pixel and its bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
pub(super) struct RadarStore {
    pixel: u32,
    color: u32,
}

impl RadarStore {
    /// A store at aperture pixel `(x, y)`; none off the aperture, where the
    /// radar stores nothing.
    pub fn at((x, y): (u32, u32), color: [u8; 4]) -> Option<Self> {
        (x < MINIMAP_WIDTH && y < MINIMAP_HEIGHT).then_some(Self {
            pixel: x | y << 16,
            color: u32::from_le_bytes(color),
        })
    }

    #[cfg(test)]
    pub fn pixel(self) -> (u32, u32) {
        (self.pixel & 0xffff, self.pixel >> 16)
    }

    #[cfg(test)]
    pub fn color(self) -> [u8; 4] {
        self.color.to_le_bytes()
    }
}

/// Whose shroud and fog the generated terrain shows.
#[derive(Debug, Clone, Copy)]
pub(super) enum RadarVisibility<'a> {
    /// No viewer, or full visibility: every cell revealed.
    Revealed,
    /// The viewer's cell flags (`OwnerVisibility::cells_raw`), row-major,
    /// and the plane's size in cells.
    Plane { flags: &'a [u8], size: (u16, u16) },
    /// A viewer without a plane has revealed nothing.
    Shrouded,
}

/// `compose.wgsl`'s `Params`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Params {
    offset: [i32; 2],
    size: [i32; 2],
    hd_origin: [i32; 2],
    hd_size: [i32; 2],
    plane_size: [u32; 2],
    hd_scale: u32,
    visibility: u32,
    revealed: u32,
    gap_covered: u32,
    gap_fog: u32,
    pad: u32,
}

pub(crate) fn shader_source() -> String {
    format!(
        "const APERTURE = vec2f({MINIMAP_WIDTH}.0, {MINIMAP_HEIGHT}.0);\n{}\n{}",
        crate::render::webgl::srgb_wgsl(),
        include_str!("compose.wgsl")
    )
}

/// A generated surface's R5G6B5 words and the cell each pixel reads, as GPU
/// textures. The words follow the terrain; the cells only the geometry.
pub(super) struct SurfaceTextures {
    words: wgpu::Texture,
    cells: wgpu::Texture,
}

impl SurfaceTextures {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        geometry: NativeRadarSurfaceGeometry,
        words: &[u16],
    ) -> Self {
        let (width, height) = geometry.generated_size();
        let size = [width.max(1) as u32, height.max(1) as u32];
        let cells: Vec<u32> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|pixel| {
                let (rx, ry) = geometry.surface_pixel_to_visibility_cell(pixel);
                u32::from(rx) | u32::from(ry) << 16
            })
            .collect();
        let textures = Self {
            words: input_texture(
                device,
                "Radar terrain words",
                size,
                wgpu::TextureFormat::R16Uint,
            ),
            cells: input_texture(
                device,
                "Radar pixel cells",
                size,
                wgpu::TextureFormat::R32Uint,
            ),
        };
        write_rows(
            queue,
            &textures.cells,
            0,
            size[0],
            bytemuck::cast_slice(&cells),
        );
        textures.write_words(queue, words);
        textures
    }

    /// One-texel stand-ins, bound where no surface is installed.
    fn empty(device: &wgpu::Device) -> Self {
        Self {
            words: input_texture(
                device,
                "No radar terrain",
                [1, 1],
                wgpu::TextureFormat::R16Uint,
            ),
            cells: input_texture(
                device,
                "No radar cells",
                [1, 1],
                wgpu::TextureFormat::R32Uint,
            ),
        }
    }

    pub fn write_words(&self, queue: &wgpu::Queue, words: &[u16]) {
        let size = self.words.size();
        debug_assert_eq!(words.len(), (size.width * size.height) as usize);
        write_rows(
            queue,
            &self.words,
            0,
            size.width,
            bytemuck::cast_slice(words),
        );
    }

    fn views(&self) -> [wgpu::TextureView; 2] {
        [view(&self.words), view(&self.cells)]
    }
}

fn input_texture(
    device: &wgpu::Device,
    label: &'static str,
    [width, height]: [u32; 2],
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Write whole rows of `texture`, `width` texels each, from `first_row` on.
fn write_rows(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    first_row: u32,
    width: u32,
    bytes: &[u8],
) {
    let texel = texture
        .format()
        .block_copy_size(None)
        .expect("plain colour format");
    let height = bytes.len() as u32 / (width * texel);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            origin: wgpu::Origin3d {
                x: 0,
                y: first_row,
                z: 0,
            },
            ..texture.as_image_copy()
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * texel),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

fn view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&Default::default())
}

/// The composition programs.
struct Pipelines {
    inputs: wgpu::BindGroupLayout,
    composite: wgpu::BindGroupLayout,
    terrain: wgpu::RenderPipeline,
    store: wgpu::RenderPipeline,
    present: wgpu::RenderPipeline,
    hd: wgpu::RenderPipeline,
}

impl Pipelines {
    fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Radar composition"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        let uint = wgpu::TextureSampleType::Uint;
        let inputs = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Radar inputs"),
            entries: &[
                uniform_entry(0),
                texture_entry(1, uint),
                texture_entry(2, uint),
                texture_entry(3, uint),
                texture_entry(4, uint),
                texture_entry(5, uint),
            ],
        });
        let composite = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Radar composite"),
            entries: &[texture_entry(0, uint)],
        });
        let store_attributes = wgpu::vertex_attr_array![0 => Uint32x2];
        let store_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: size_of::<RadarStore>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &store_attributes,
        })];
        let pipeline = |label,
                        layouts: &[Option<&wgpu::BindGroupLayout>],
                        vertex: &'static str,
                        fragment: &'static str,
                        format: wgpu::TextureFormat| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: layouts,
                immediate_size: 0,
            });
            let stores = vertex == "vs_store";
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    buffers: if stores { &store_buffers[..] } else { &[] },
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: if stores {
                        wgpu::PrimitiveTopology::TriangleStrip
                    } else {
                        wgpu::PrimitiveTopology::TriangleList
                    },
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let both = [Some(&inputs), Some(&composite)];
        Self {
            terrain: pipeline(
                "Radar terrain (655C50)",
                &[Some(&inputs)],
                "vs_fullscreen",
                "fs_terrain",
                COMPOSITE_FORMAT,
            ),
            store: pipeline(
                "Radar stores",
                &[],
                "vs_store",
                "fs_store",
                COMPOSITE_FORMAT,
            ),
            present: pipeline(
                "Radar picture",
                &both,
                "vs_fullscreen",
                "fs_present",
                PICTURE_FORMAT,
            ),
            hd: pipeline(
                "HD radar picture",
                &both,
                "vs_fullscreen",
                "fs_hd",
                PICTURE_FORMAT,
            ),
            inputs,
            composite,
        }
    }
}

/// The GPU side of one minimap: its programs, inputs, the native composite,
/// and the pictures drawn from it.
pub(super) struct RadarCompositor {
    pipelines: Pipelines,
    params: Params,
    params_buffer: wgpu::Buffer,
    composite: wgpu::Texture,
    composite_group: wgpu::BindGroup,
    /// The native 140x108 picture.
    picture: BatchTexture,
    /// The generated surface; none for the mapless fallback.
    surface: Option<SurfaceTextures>,
    /// The terrain revision `surface` holds.
    terrain_revision: u64,
    hd: Option<HdRadar>,
    plane: wgpu::Texture,
    /// The flags `plane` holds; the viewer's flags change every tick, but
    /// only a few rows of them.
    plane_flags: Vec<u8>,
    /// Stand-ins bound where no surface or HD radar is installed.
    empty: SurfaceTextures,
    /// Rebuilt after an input texture is replaced.
    inputs: Option<wgpu::BindGroup>,
    stores: InstanceUpload,
    store_count: u32,
    /// Whether the pictures must be drawn again.
    pending: bool,
}

impl RadarCompositor {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, batch: &BatchRenderer) -> Self {
        let pipelines = Pipelines::new(device);
        let composite = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Radar composite"),
            size: wgpu::Extent3d {
                width: MINIMAP_WIDTH,
                height: MINIMAP_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COMPOSITE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let composite_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Radar composite"),
            layout: &pipelines.composite,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view(&composite)),
            }],
        });
        let params = Params {
            revealed: crate::sim::vision::FLAG_REVEALED.into(),
            gap_covered: crate::sim::vision::FLAG_GAP_COVERED.into(),
            gap_fog: crate::sim::vision::FLAG_GAP_FOG.into(),
            ..Params::zeroed()
        };
        let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Radar parameters"),
            size: size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let compositor = Self {
            pipelines,
            params,
            params_buffer,
            composite,
            composite_group,
            picture: batch.create_render_target_texture(
                device,
                MINIMAP_WIDTH,
                MINIMAP_HEIGHT,
                PICTURE_FORMAT,
            ),
            surface: None,
            terrain_revision: 0,
            hd: None,
            plane: input_texture(
                device,
                "Radar cell flags",
                [1, 1],
                wgpu::TextureFormat::R8Uint,
            ),
            plane_flags: Vec::new(),
            empty: SurfaceTextures::empty(device),
            inputs: None,
            stores: InstanceUpload::default(),
            store_count: 0,
            pending: true,
        };
        compositor.write_params(queue);
        compositor
    }

    fn write_params(&self, queue: &wgpu::Queue) {
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&self.params));
    }

    /// Install `terrain`'s generated surface, or none for the mapless
    /// fallback. The HD radar goes with the old surface.
    pub fn set_surface(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        terrain: Option<&NativeRadarTerrainSurface>,
    ) {
        self.surface = terrain.map(|terrain| {
            SurfaceTextures::new(
                device,
                queue,
                terrain.geometry(),
                terrain.generated_rgb565(),
            )
        });
        self.terrain_revision = terrain.map_or(0, NativeRadarTerrainSurface::revision);
        let geometry = terrain.map(NativeRadarTerrainSurface::geometry);
        let offset = geometry.map_or((0, 0), NativeRadarSurfaceGeometry::aperture_offset);
        let size = geometry.map_or((0, 0), NativeRadarSurfaceGeometry::generated_size);
        self.params.offset = [offset.0, offset.1];
        self.params.size = [size.0, size.1];
        self.hd = None;
        self.params.hd_scale = 0;
        self.write_params(queue);
        self.inputs = None;
        self.pending = true;
    }

    /// Upload `terrain`'s words, native and HD, where they changed.
    pub fn refresh_terrain(&mut self, queue: &wgpu::Queue, terrain: &NativeRadarTerrainSurface) {
        if let Some(surface) = &self.surface
            && self.terrain_revision != terrain.revision()
        {
            surface.write_words(queue, terrain.generated_rgb565());
            self.terrain_revision = terrain.revision();
        }
        if let (Some(hd), Some(words)) = (&mut self.hd, terrain.hd_rgb565()) {
            hd.refresh(queue, words);
        }
    }

    /// Draw VERA's HD radar at `scale` from `terrain`'s HD words, or not.
    pub fn set_hd(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        hd: Option<(u32, &NativeRadarTerrainSurface)>,
    ) {
        self.hd = hd.and_then(|(scale, terrain)| {
            let words = terrain.hd_rgb565()?;
            Some(HdRadar::new(
                device,
                queue,
                batch,
                terrain.geometry(),
                scale,
                words,
            ))
        });
        let (scale, origin, size) = self.hd.as_ref().map_or((0, [0; 2], [0; 2]), |hd| {
            (hd.scale(), hd.origin(), hd.size())
        });
        self.params.hd_scale = scale;
        self.params.hd_origin = origin;
        self.params.hd_size = size;
        self.write_params(queue);
        self.inputs = None;
        self.pending = true;
    }

    pub fn hd(&self) -> Option<&HdRadar> {
        self.hd.as_ref()
    }

    /// Show `visibility`'s shroud and fog from the next composition on.
    pub fn set_visibility(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        visibility: RadarVisibility<'_>,
    ) {
        self.params.visibility = match visibility {
            RadarVisibility::Revealed => 0,
            RadarVisibility::Plane { flags, size } => {
                let size = [u32::from(size.0), u32::from(size.1)];
                debug_assert_eq!(flags.len(), (size[0] * size[1]) as usize);
                let texture_size = [size[0].max(1), size[1].max(1)];
                let current = self.plane.size();
                if [current.width, current.height] != texture_size {
                    self.plane = input_texture(
                        device,
                        "Radar cell flags",
                        texture_size,
                        wgpu::TextureFormat::R8Uint,
                    );
                    self.plane_flags.clear();
                    self.inputs = None;
                }
                self.write_plane(queue, flags, size[0]);
                self.params.plane_size = size;
                1
            }
            RadarVisibility::Shrouded => 2,
        };
        self.write_params(queue);
    }

    /// Upload the rows of `flags` that differ from the plane's.
    fn write_plane(&mut self, queue: &wgpu::Queue, flags: &[u8], width: u32) {
        if flags.is_empty() {
            return;
        }
        if self.plane_flags.len() != flags.len() {
            write_rows(queue, &self.plane, 0, width, flags);
        } else {
            let row = width as usize;
            let rows = flags.len() / row;
            let differs = |y: usize| flags[y * row..][..row] != self.plane_flags[y * row..][..row];
            let mut y = 0;
            while y < rows {
                if !differs(y) {
                    y += 1;
                    continue;
                }
                let start = y;
                while y < rows && differs(y) {
                    y += 1;
                }
                let bytes = &flags[start * row..y * row];
                write_rows(queue, &self.plane, start as u32, width, bytes);
            }
        }
        self.plane_flags.clear();
        self.plane_flags.extend_from_slice(flags);
    }

    /// The mapless fallback's terrain, composed on the CPU, as the base the
    /// stores are drawn over.
    pub fn set_fallback_base(&mut self, queue: &wgpu::Queue, rgba: &[u8]) {
        debug_assert!(self.surface.is_none());
        write_rows(queue, &self.composite, 0, MINIMAP_WIDTH, rgba);
    }

    pub fn set_stores(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stores: &[RadarStore],
    ) {
        self.store_count = stores.len() as u32;
        if !stores.is_empty() {
            self.stores
                .write(device, queue, "Radar stores", bytemuck::cast_slice(stores));
        }
    }

    /// Draw the pictures again with the next [`Self::encode`].
    pub fn request(&mut self) {
        self.pending = true;
    }

    /// Compose and present a requested update into `encoder`.
    pub fn encode(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) {
        if !std::mem::take(&mut self.pending) {
            return;
        }
        let inputs = self.inputs.get_or_insert_with(|| {
            let [words, cells] = self.surface.as_ref().unwrap_or(&self.empty).views();
            let [hd_words, hd_cells] = self
                .hd
                .as_ref()
                .map_or(&self.empty, HdRadar::textures)
                .views();
            let plane = view(&self.plane);
            let entries = [&words, &cells, &plane, &hd_words, &hd_cells];
            let mut group = vec![wgpu::BindGroupEntry {
                binding: 0,
                resource: self.params_buffer.as_entire_binding(),
            }];
            group.extend(
                entries
                    .iter()
                    .zip(1..)
                    .map(|(view, binding)| wgpu::BindGroupEntry {
                        binding,
                        resource: wgpu::BindingResource::TextureView(view),
                    }),
            );
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Radar inputs"),
                layout: &self.pipelines.inputs,
                entries: &group,
            })
        });
        let composite = view(&self.composite);
        {
            // The generated surface covers the aperture; the fallback's base
            // was written into the composite.
            let load = if self.surface.is_some() {
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
            } else {
                wgpu::LoadOp::Load
            };
            let mut pass = begin_pass(encoder, "Radar composite", &composite, load);
            if self.surface.is_some() {
                pass.set_pipeline(&self.pipelines.terrain);
                pass.set_bind_group(0, &*inputs, &[]);
                pass.draw(0..3, 0..1);
            }
            if let Some(stores) = self.stores.buffer().filter(|_| self.store_count > 0) {
                pass.set_pipeline(&self.pipelines.store);
                pass.set_vertex_buffer(0, stores.slice(..));
                pass.draw(0..4, 0..self.store_count);
            }
        }
        let pictures = std::iter::once((&self.pipelines.present, &self.picture)).chain(
            self.hd
                .as_ref()
                .map(|hd| (&self.pipelines.hd, hd.picture())),
        );
        for (pipeline, picture) in pictures {
            let mut pass = begin_pass(
                encoder,
                "Radar picture",
                &picture.view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            );
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &*inputs, &[]);
            pass.set_bind_group(1, &self.composite_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// The native picture, 140x108.
    pub fn picture(&self) -> &BatchTexture {
        &self.picture
    }
}

fn begin_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    label: &'static str,
    target: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

#[cfg(test)]
#[path = "compose_gpu_tests.rs"]
mod gpu_tests;
