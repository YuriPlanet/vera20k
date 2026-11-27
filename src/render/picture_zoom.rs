//! VERA-only, experimental (`docs/picture-model-art.md`, chain 6): the HD
//! twins of picture pages on the GPU.
//!
//! Art above 1x keeps an HD twin of each of its atlas pages
//! (`picture_atlas::HdPage`) at the same normalised coordinates, so a world
//! instance's UV addresses its HD rectangle unchanged. A frame whose camera
//! zooms in binds the twin in place of the 1x page
//! (`SpriteAtlas::bind_one_pass_hd`, `SelectionOverlay::bind_one_pass_hd`,
//! `BeaconArt::bind_one_pass_hd`), and the ordinary blitters sample it at
//! the level the zoom needs (`sprite_source.wgsl`, `docs/zoom-one-pass.md`).
//! The WebGL2 fallback builds no HD pages.

use crate::render::batch::BatchRenderer;
use crate::render::picture_atlas::HdPage;

/// One HD page on the GPU: premultiplied encoded colour with mipmaps down to
/// one texel per game pixel, its level-0 index plane, and its house layer
/// with the colour's mipmaps when a frame on it has one
/// (`picture_atlas::HdSprite::house`).
pub(crate) struct PictureHdPage {
    color: wgpu::TextureView,
    indices: wgpu::TextureView,
    house: Option<wgpu::TextureView>,
}

impl PictureHdPage {
    /// A page drawn on the GPU (a model page's HD twin,
    /// `render::model_draw`): encoded colour that is opaque or empty, so
    /// already premultiplied, with one level, and its index plane.
    pub(crate) fn from_views(color: wgpu::TextureView, indices: wgpu::TextureView) -> Self {
        Self {
            color,
            indices,
            house: None,
        }
    }

    /// VERA-only, experimental (`docs/zoom-one-pass.md`): this twin as a
    /// group-1 page binding of the world blitters, beside `low_page`, the 1x
    /// page's colour (`BatchRenderer::create_picture_hd_binding`).
    pub(crate) fn one_pass_binding(
        &self,
        device: &wgpu::Device,
        batch: &BatchRenderer,
        low_page: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        batch.create_picture_hd_binding(
            device,
            &self.color,
            &self.indices,
            self.house.as_ref(),
            low_page,
        )
    }

    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue, page: &HdPage) -> Self {
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
                "Picture HD page",
                wgpu::TextureFormat::Rgba8Unorm,
                levels,
                &chain,
            ),
            indices: texture(
                "Picture HD page indices",
                wgpu::TextureFormat::R8Uint,
                1,
                &page.indices,
            ),
            house: page.house_mip_chain().map(|chain| {
                texture(
                    "Picture HD page house layer",
                    wgpu::TextureFormat::Rg8Unorm,
                    levels,
                    &chain,
                )
            }),
        }
    }
}
