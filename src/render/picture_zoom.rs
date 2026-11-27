//! VERA-only, experimental (`docs/picture-model-art.md`, `docs/zoom-one-pass.md`):
//! picture pages on the GPU.
//!
//! A picture frame exists once on the GPU, on a page at the picture's own
//! resolution (`picture_atlas::PicturePage`) with mip levels down past one
//! texel per game pixel. Its page binding is the only one it has: the
//! ordinary blitters sample it at the level the camera needs at every zoom
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
