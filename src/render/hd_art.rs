//! VERA's HD art: 2x upscaled building frames drawn over the
//! wheel-zoomed world.
//!
//! VERA-only presentation extension (HD art (`[VERA] HdArt=`, `render::hd_switch`), off by default). The
//! world is still composed at native pixels, exactly as before; when the
//! wheel zoom magnifies it, this pass replaces the pixels of the zoomed copy
//! that the native world still shows as a building frame's own colour with
//! that frame's HD art (`hd_art.wgsl`). Nothing reaches `sim/`, a save, a
//! replay or the network.
//!
//! The art lives in a pack folder (`hd_art/pack` beside the executable, or
//! `VERA20K_HD_ART_PACK`): `<hash>.png` per frame, 2x the frame's stored
//! rectangle, RGB in the pack's `reference.pal` and alpha for coverage. The
//! hash ([`frame_hash`]) is the FNV-1a of the frame's size and palette
//! indices, so a frame finds its art whichever type, theater or mod draws it.
//! `VERA20K_HD_DUMP=<folder>` writes `<hash>.idx` (u16 width, u16 height,
//! indices) for every building frame the pack lacks, for the offline upscaler
//! (`hd_art/tools`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bytemuck::{Pod, Zeroable};

use super::sprite_atlas::{ShpSpriteKey, SpriteAtlas};

/// Native pixels per side of the index page; the HD page is twice that.
const PAGE: u32 = 2048;

/// New pack files read in one frame, so a screen of new buildings arrives
/// over a few frames instead of stalling one.
const LOADS_PER_FRAME: usize = 24;

/// The index page has no room left for a frame.
struct PageFull;

/// One visible atlas piece: where its frame is in the sprite atlas, and where
/// the SHP blitter put it in the native composition.
pub(crate) struct HdArtPiece {
    pub page: usize,
    pub uv_origin: [f32; 2],
    pub origin: [f32; 2],
    pub size: [f32; 2],
    pub source_palette: u32,
}

/// FNV-1a 64 of a frame: width and height (u16 little-endian), then its
/// palette indices row by row.
pub(crate) fn frame_hash(width: u32, height: u32, indices: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |byte: u8| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for byte in (width as u16)
        .to_le_bytes()
        .into_iter()
        .chain((height as u16).to_le_bytes())
    {
        eat(byte);
    }
    for &byte in indices {
        eat(byte);
    }
    hash
}

/// The pack folder: `VERA20K_HD_ART_PACK`, else the selected mod's
/// `hd_art/pack` (`Mods/<mod>/hd_art/pack`, docs/hd-art.md): an HD pack
/// installs like any mod. None without either.
pub(crate) fn pack_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("VERA20K_HD_ART_PACK") {
        return Some(PathBuf::from(dir));
    }
    crate::map::source::mod_dir()
        .map(|mod_dir| mod_dir.join("hd_art").join("pack"))
        .filter(|dir| dir.is_dir())
}

static PACK_AVAILABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Look for the pack again: once a start has chosen its mod
/// (`app::initialize`), and in tests.
pub(crate) fn refresh_pack_available() {
    let found = pack_dir().is_some_and(|dir| dir.is_dir());
    PACK_AVAILABLE.store(found, std::sync::atomic::Ordering::Relaxed);
}

/// Whether this start has an HD art pack ([`refresh_pack_available`]):
/// HD art can be on only with one.
pub(crate) fn pack_available() -> bool {
    PACK_AVAILABLE.load(std::sync::atomic::Ordering::Relaxed)
}

/// A folder of the pack (`terrain`, `sidebar`).
pub(crate) fn pack_subdir(name: &str) -> Option<PathBuf> {
    Some(pack_dir()?.join(name))
}

static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// HD art's state (`render::hd_switch`), set at the start of each frame.
pub(crate) fn set_active(on: bool) {
    ACTIVE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether HD art is on this frame: the sidebar then draws its HD pictures.
pub(crate) fn sidebar_active() -> bool {
    ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

fn load_reference_palette(dir: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(dir.join("reference.pal")).ok()?;
    if bytes.len() < 768 {
        return None;
    }
    // 6-bit VGA entries, as the game's PAL files.
    let mut rgba = Vec::with_capacity(1024);
    for entry in bytes[..768].chunks_exact(3) {
        for &c in entry {
            rgba.push(((u32::from(c) * 255) / 63).min(255) as u8);
        }
        rgba.push(255);
    }
    Some(rgba)
}

/// One HD draw: a building frame instance placed in the native composition.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub(crate) struct HdArtDraw {
    origin: [f32; 2],
    size: [f32; 2],
    page_origin: [u32; 2],
    row: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    frame: [f32; 4],
}

struct Gpu {
    indices: wgpu::Texture,
    art: wgpu::Texture,
    reference: wgpu::Texture,
    rows: Option<(wgpu::Texture, usize)>,
    layout: wgpu::BindGroupLayout,
    pipeline: Option<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    instances: wgpu::Buffer,
    instance_capacity: usize,
}

/// The pack's frames resident on the GPU, keyed by the atlas entries that
/// show them.
pub(crate) struct HdArt {
    dir: PathBuf,
    dump: Option<PathBuf>,
    reference: Vec<u8>,
    generation: (usize, usize, usize),
    /// Atlas entry (page, uv origin bits) of every building frame → its key.
    candidates: HashMap<(u8, u32, u32), ShpSpriteKey>,
    /// Atlas entry → the frame's native rectangle in the index page, or
    /// `None` when the pack has no art for it.
    entries: HashMap<(u8, u32, u32), Option<[u32; 4]>>,
    /// Frame hash → its rectangle, or `None` when the pack has no art for it.
    frames: HashMap<u64, Option<[u32; 4]>>,
    shelf: [u32; 3],
    gpu: Option<Gpu>,
}

impl HdArt {
    /// The pack, or `None` when there is no pack folder with a reference
    /// palette: the pass then never runs.
    pub(crate) fn open() -> Option<Self> {
        let dir = pack_dir()?;
        let reference = load_reference_palette(&dir)?;
        log::info!("HD art pack: {}", dir.display());
        Some(Self {
            dir,
            dump: std::env::var("VERA20K_HD_DUMP").ok().map(PathBuf::from),
            reference,
            generation: (0, 0, 0),
            candidates: HashMap::new(),
            entries: HashMap::new(),
            frames: HashMap::new(),
            shelf: [0, 0, 0],
            gpu: None,
        })
    }

    fn key(page: u8, uv: [f32; 2]) -> (u8, u32, u32) {
        (page, uv[0].to_bits(), uv[1].to_bits())
    }

    /// Bring the art of `pieces` (this frame's visible pieces) up: each
    /// building frame among them that the pack has art for is read and
    /// uploaded once, at most [`LOADS_PER_FRAME`] new files a frame. Only
    /// frames on screen are kept, since the atlas holds every animation of
    /// every building type, far more than one page; a full page starts over.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &SpriteAtlas,
        pieces: &[HdArtPiece],
    ) {
        self.ensure_gpu(device, queue);
        if self.generation != atlas.generation() {
            self.generation = atlas.generation();
            self.candidates = atlas
                .building_frames()
                .map(|(key, entry)| (Self::key(entry.page, entry.uv_origin), key.clone()))
                .collect();
        }
        let mut uploads = Vec::new();
        let mut loads = 0;
        for piece in pieces {
            let Ok(page) = u8::try_from(piece.page) else {
                continue;
            };
            let entry_key = Self::key(page, piece.uv_origin);
            if self.entries.contains_key(&entry_key) {
                continue;
            }
            let Some(key) = self.candidates.get(&entry_key) else {
                continue;
            };
            let Some(entry) = atlas.get(key) else {
                continue;
            };
            let (width, height) = (entry.pixel_size[0] as u32, entry.pixel_size[1] as u32);
            let indices = atlas.frame_indices(key).filter(|indices| {
                width > 0 && height > 0 && indices.len() == (width * height) as usize
            });
            let Some(indices) = indices else {
                self.entries.insert(entry_key, None);
                continue;
            };
            let hash = frame_hash(width, height, &indices);
            let rect = match self.frames.get(&hash) {
                Some(rect) => *rect,
                None => {
                    if loads >= LOADS_PER_FRAME {
                        continue;
                    }
                    loads += 1;
                    match self.load_frame(hash, width, height, &indices, &mut uploads) {
                        Ok(rect) => {
                            self.frames.insert(hash, rect);
                            rect
                        }
                        Err(PageFull) => {
                            // Start the page over: what is on screen now
                            // comes back over the next frames.
                            log::info!("HD art page full; reloading the frames on screen");
                            self.entries.retain(|_, rect| rect.is_none());
                            self.frames.retain(|_, rect| rect.is_none());
                            self.shelf = [0, 0, 0];
                            uploads.clear();
                            continue;
                        }
                    }
                }
            };
            self.entries.insert(entry_key, rect);
        }
        let gpu = self.gpu.as_ref().expect("HD art GPU resources");
        for (rect, indices, art) in uploads {
            super::atlas_growth::write_texels(
                queue,
                &gpu.indices,
                [rect[0], rect[1]],
                [rect[2], rect[3]],
                1,
                &indices,
            );
            super::atlas_growth::write_texels(
                queue,
                &gpu.art,
                [rect[0] * 2, rect[1] * 2],
                [rect[2] * 2, rect[3] * 2],
                4,
                &art,
            );
        }
        self.upload_rows(device, queue, atlas);
    }

    /// Read `<hash>.png` and place it: `Ok(None)` when the pack lacks it
    /// (the frame is dumped then), `Err` when the page is full.
    fn load_frame(
        &mut self,
        hash: u64,
        width: u32,
        height: u32,
        indices: &[u8],
        uploads: &mut Vec<([u32; 4], Vec<u8>, Vec<u8>)>,
    ) -> Result<Option<[u32; 4]>, PageFull> {
        let name = format!("{hash:016x}");
        let art = std::fs::read(self.dir.join(format!("{name}.png")))
            .ok()
            .and_then(|bytes| {
                image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()
            })
            .map(|image| image.into_rgba8());
        let Some(art) = art.filter(|art| art.width() == width * 2 && art.height() == height * 2)
        else {
            if let Some(dump) = &self.dump {
                let mut bytes = Vec::with_capacity(4 + indices.len());
                bytes.extend_from_slice(&(width as u16).to_le_bytes());
                bytes.extend_from_slice(&(height as u16).to_le_bytes());
                bytes.extend_from_slice(indices);
                let _ = std::fs::create_dir_all(dump);
                let _ = std::fs::write(dump.join(format!("{name}.idx")), bytes);
            }
            return Ok(None);
        };
        if width >= PAGE || height >= PAGE {
            return Ok(None);
        }
        let rect = self.place(width, height).ok_or(PageFull)?;
        uploads.push((rect, indices.to_vec(), art.into_raw()));
        Ok(Some(rect))
    }

    /// Shelf placement in the native index page, one pixel apart.
    fn place(&mut self, width: u32, height: u32) -> Option<[u32; 4]> {
        let [x, y, row] = &mut self.shelf;
        if *x + width > PAGE {
            *y += *row + 1;
            *x = 0;
            *row = 0;
        }
        if *y + height > PAGE {
            return None;
        }
        let rect = [*x, *y, width, height];
        *x += width + 1;
        *row = (*row).max(height);
        Some(rect)
    }

    fn upload_rows(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, atlas: &SpriteAtlas) {
        let rows = atlas.palette_rows();
        let gpu = self.gpu.as_mut().expect("HD art GPU resources");
        if rows.is_empty()
            || gpu
                .rows
                .as_ref()
                .is_some_and(|(_, count)| *count == rows.len())
        {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("HD art palette rows"),
            size: wgpu::Extent3d {
                width: 256,
                height: rows.len() as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (row, colors) in rows.iter().enumerate() {
            super::atlas_growth::write_texels(
                queue,
                &texture,
                [0, row as u32],
                [256, 1],
                4,
                colors,
            );
        }
        gpu.rows = Some((texture, rows.len()));
    }

    fn ensure_gpu(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.gpu.is_some() {
            return;
        }
        let texture = |label, size: u32, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let indices = texture("HD art native indices", PAGE, wgpu::TextureFormat::R8Uint);
        let art = texture("HD art", PAGE * 2, wgpu::TextureFormat::Rgba8UnormSrgb);
        let reference = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("HD art reference palette"),
            size: wgpu::Extent3d {
                width: 256,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        super::atlas_growth::write_texels(queue, &reference, [0, 0], [256, 1], 4, &self.reference);
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
            ty,
            count: None,
        };
        let tex = |sample_type| wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let unfiltered = wgpu::TextureSampleType::Float { filterable: false };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("HD art"),
            entries: &[
                entry(0, tex(unfiltered)),
                entry(1, tex(wgpu::TextureSampleType::Uint)),
                entry(2, tex(wgpu::TextureSampleType::Float { filterable: true })),
                entry(
                    3,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(4, tex(unfiltered)),
                entry(5, tex(unfiltered)),
                entry(
                    6,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("HD art"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("HD art"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        self.gpu = Some(Gpu {
            indices,
            art,
            reference,
            rows: None,
            layout,
            pipeline: None,
            pipeline_layout,
            shader,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("HD art"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("HD art"),
                size: std::mem::size_of::<Params>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            instances: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("HD art draws"),
                size: 256 * std::mem::size_of::<HdArtDraw>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            instance_capacity: 256,
        });
    }

    /// The HD draw for `piece`, or `None` when its frame has no resident art.
    pub(crate) fn placement(&self, piece: &HdArtPiece) -> Option<HdArtDraw> {
        let key = Self::key(u8::try_from(piece.page).ok()?, piece.uv_origin);
        let rect = (*self.entries.get(&key)?)?;
        if piece.source_palette == 0 || piece.size != [rect[2] as f32, rect[3] as f32] {
            return None;
        }
        Some(HdArtDraw {
            origin: piece.origin,
            size: piece.size,
            page_origin: [rect[0], rect[1]],
            row: piece.source_palette - 1,
            _pad: 0,
        })
    }

    /// Draw `draws` into `target` (the zoomed frame, `scale` frame pixels
    /// per native pixel) over `scissor`, reading the native `composition`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        composition: &wgpu::Texture,
        scale: f32,
        scissor: [u32; 4],
        draws: &[HdArtDraw],
        debug: bool,
    ) {
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        let Some((rows, _)) = gpu.rows.as_ref() else {
            return;
        };
        if draws.is_empty() || scissor[2] == 0 || scissor[3] == 0 {
            return;
        }
        let format = target.format();
        if gpu
            .pipeline
            .as_ref()
            .is_none_or(|(cached, _)| *cached != format)
        {
            gpu.pipeline = Some((
                format,
                create_pipeline(device, &gpu.pipeline_layout, &gpu.shader, format),
            ));
        }
        if draws.len() > gpu.instance_capacity {
            gpu.instance_capacity = draws.len().next_power_of_two();
            gpu.instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("HD art draws"),
                size: (gpu.instance_capacity * std::mem::size_of::<HdArtDraw>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        queue.write_buffer(&gpu.instances, 0, bytemuck::cast_slice(draws));
        let size = target.size();
        let params = Params {
            frame: [
                size.width as f32,
                size.height as f32,
                scale,
                if debug { 1.0 } else { 0.0 },
            ],
        };
        queue.write_buffer(&gpu.uniform, 0, bytemuck::bytes_of(&params));
        let view = |texture: &wgpu::Texture| texture.create_view(&Default::default());
        let views = [
            view(composition),
            view(&gpu.indices),
            view(&gpu.art),
            view(rows),
            view(&gpu.reference),
        ];
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("HD art"),
            layout: &gpu.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&gpu.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&views[3]),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&views[4]),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: gpu.uniform.as_entire_binding(),
                },
            ],
        });
        let target_view = target.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("HD art"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
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
        let right = (scissor[0] + scissor[2]).min(size.width);
        let bottom = (scissor[1] + scissor[3]).min(size.height);
        if right <= scissor[0] || bottom <= scissor[1] {
            return;
        }
        pass.set_scissor_rect(
            scissor[0],
            scissor[1],
            right - scissor[0],
            bottom - scissor[1],
        );
        pass.set_pipeline(&gpu.pipeline.as_ref().expect("pipeline").1);
        pass.set_bind_group(0, &group, &[]);
        pass.set_vertex_buffer(0, gpu.instances.slice(..));
        pass.draw(0..4, 0..draws.len() as u32);
    }
}

fn shader_source() -> String {
    format!(
        "{}\n{}",
        super::webgl::srgb_wgsl(),
        include_str!("hd_art.wgsl")
    )
}

fn create_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let attributes =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Uint32x2, 3 => Uint32];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("HD art"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<HdArtDraw>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &attributes,
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;

    #[test]
    fn hd_art_shader_parses_and_validates() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(&shader_source()).expect("HD art shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("HD art shader validates");
    }

    /// The offline upscaler (`hd_art/tools`) names files by the same hash;
    /// this pins its byte order.
    #[test]
    fn frame_hash_covers_size_and_indices() {
        // The same value the Python tool computes for this frame.
        assert_eq!(frame_hash(2, 1, &[5, 6]), 0xf10c_ed57_af42_a1d5);
        assert_ne!(frame_hash(2, 1, &[5, 6]), frame_hash(1, 2, &[5, 6]));
        assert_ne!(frame_hash(2, 1, &[5, 6]), frame_hash(2, 1, &[6, 5]));
    }

    /// An HD pack installs as a mod: the selected mod's `hd_art/pack` is the
    /// pack, and a mod without one, or no mod, leaves none.
    #[test]
    fn the_selected_mods_pack_is_the_pack() {
        if std::env::var_os("VERA20K_HD_ART_PACK").is_some() {
            return;
        }
        let root = std::env::temp_dir().join(format!("vera-hd-pack-{}", std::process::id()));
        let with_pack = root.join("Mods").join("HD Pack");
        std::fs::create_dir_all(with_pack.join("hd_art").join("pack")).unwrap();
        let without = root.join("Mods").join("Gameplay");
        std::fs::create_dir_all(&without).unwrap();

        crate::map::source::set_mod_dir(Some(with_pack.clone()));
        refresh_pack_available();
        assert!(pack_available());
        assert_eq!(pack_dir(), Some(with_pack.join("hd_art").join("pack")));
        assert_eq!(
            pack_subdir("terrain"),
            Some(with_pack.join("hd_art").join("pack").join("terrain"))
        );
        crate::map::source::set_mod_dir(Some(without));
        refresh_pack_available();
        assert!(!pack_available());
        assert_eq!(pack_dir(), None);
        crate::map::source::set_mod_dir(None);
        refresh_pack_available();
        assert!(!pack_available());
        assert_eq!(pack_dir(), None);
        let _ = std::fs::remove_dir_all(root);
    }
}
