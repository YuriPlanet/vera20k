//! VERA-only, experimental (`docs/picture-model-art.md`, `docs/zoom-one-pass.md`):
//! picture pages on the GPU.
//!
//! A picture frame is on the GPU for good at game pixels, on a page
//! (`picture_atlas::PicturePage`) with a mip level past one texel per game
//! pixel; while the camera zooms in, frames of art above 1x are streamed
//! onto HD pages at the picture's own resolution with their mips
//! (`render::picture_stream`). A page's binding is the only one it has: the
//! ordinary blitters sample it at the level the camera needs
//! (`sprite_source.wgsl`), and read the owner's house colour from the
//! renderer's house ramps texture (`BatchRenderer::write_picture_ramps`).

use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::picture_atlas::PicturePage;

/// One picture page on the GPU: premultiplied encoded colour with its mip
/// levels, its level-0 index plane, and its house layer with the colour's
/// levels when a frame on it has one (`picture_atlas::PageSprite::house`).
pub(crate) struct PictureHdPage {
    color: wgpu::TextureView,
    indices: wgpu::TextureView,
    house: Option<wgpu::TextureView>,
    size: [u32; 2],
}

impl PictureHdPage {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue, page: &PicturePage) -> Self {
        use wgpu::util::DeviceExt;
        let size = wgpu::Extent3d {
            width: page.size[0],
            height: page.size[1],
            depth_or_array_layers: 1,
        };
        let (chain, levels) = page.mip_chain();
        let texture = |label, format, mip_level_count, data: &[u8]| {
            device
                .create_texture_with_data(
                    queue,
                    &wgpu::TextureDescriptor {
                        label: Some(label),
                        size,
                        mip_level_count,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    data,
                )
                .create_view(&Default::default())
        };
        Self {
            // Encoded bytes, as the palette rows: the shader divides by alpha
            // and hands the result to the palette light unchanged.
            color: texture(
                "Picture page",
                wgpu::TextureFormat::Rgba8Unorm,
                levels,
                &chain,
            ),
            indices: texture(
                "Picture page indices",
                wgpu::TextureFormat::R8Uint,
                1,
                &page.indices,
            ),
            house: page.house_mip_chain().map(|chain| {
                texture(
                    "Picture page house layer",
                    wgpu::TextureFormat::Rg8Unorm,
                    levels,
                    &chain,
                )
            }),
            size: page.size,
        }
    }

    /// An empty page `side` texels square with `levels` mip levels and a
    /// house layer, written a frame at a time ([`Self::write`]): an HD page
    /// of streamed frames (`render::picture_stream`). Zeroed: no coverage.
    pub(crate) fn empty(device: &wgpu::Device, side: u32, levels: u32) -> Self {
        let texture = |label, format, mip_level_count| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: side,
                        height: side,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        Self {
            color: texture("Picture HD page", wgpu::TextureFormat::Rgba8Unorm, levels),
            indices: texture("Picture HD page indices", wgpu::TextureFormat::R8Uint, 1),
            house: Some(texture(
                "Picture HD page house layer",
                wgpu::TextureFormat::Rg8Unorm,
                levels,
            )),
            size: [side, side],
        }
    }

    /// Write a rectangle of [`Self::empty`]'s page at `origin` texels, `size`
    /// texels at level 0 (both whole multiples of the last level's texel):
    /// `color` and `house`, their `levels` mip levels one after another
    /// (`PicturePage::mip_chain`), and the level-0 `indices`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write(
        &self,
        queue: &wgpu::Queue,
        origin: [u32; 2],
        size: [u32; 2],
        levels: u32,
        color: &[u8],
        house: &[u8],
        indices: &[u8],
    ) {
        let write = |view: &wgpu::TextureView, chain: &[u8], channels: u32, levels: u32| {
            let mut offset = 0;
            for level in 0..levels {
                let [width, height] = size.map(|side| (side >> level).max(1));
                let len = (width * height * channels) as usize;
                let Some(texels) = chain.get(offset..offset + len) else {
                    return;
                };
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: view.texture(),
                        mip_level: level,
                        origin: wgpu::Origin3d {
                            x: origin[0] >> level,
                            y: origin[1] >> level,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    texels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width * channels),
                        rows_per_image: Some(height),
                    },
                    wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                );
                offset += len;
            }
        };
        write(&self.color, color, 4, levels);
        write(&self.indices, indices, 1, 1);
        if let Some(view) = &self.house {
            write(view, house, 2, levels);
        }
    }

    /// This page as the group-1 page binding every source blitter reads
    /// (`BatchRenderer::create_picture_hd_binding`), with its views and
    /// size: the page's only binding.
    pub(crate) fn texture(&self, device: &wgpu::Device, batch: &BatchRenderer) -> BatchTexture {
        BatchTexture {
            bind_group: batch.create_picture_hd_binding(
                device,
                &self.color,
                &self.indices,
                self.house.as_ref(),
            ),
            view: self.color.clone(),
            source_indices: self.indices.clone(),
            width: self.size[0],
            height: self.size[1],
        }
    }
}

/// `page` uploaded and bound: [`PictureHdPage::new`] then
/// [`PictureHdPage::texture`]. The bind group keeps the textures alive.
pub(crate) fn page_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &BatchRenderer,
    page: &PicturePage,
) -> BatchTexture {
    PictureHdPage::new(device, queue, page).texture(device, batch)
}
