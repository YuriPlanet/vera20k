//! Phobos `[SIDE] SuperWeaponSidebar.*PCX` artwork.
//!
//! Phobos loads each PCX through `PhobosPCXFile::GetSurface` and blits it
//! without scaling through `PCX::BlitToSurface` (`SWColumnClass::Draw`,
//! `ToggleSWButtonClass::Draw`). VERA packs the side images into one RGBA
//! atlas so the layer is one batch texture. Ares `SidebarPCX` superweapon
//! cameos come from the shared sidebar cameo atlas.

use std::collections::HashMap;

use crate::assets::asset_manager::AssetManager;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;
use crate::render::sidebar_cameo_atlas::NativePcxImage;
use crate::rules::ruleset::RuleSet;

const ART_PADDING: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SwSidebarArtKind {
    On,
    Off,
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy)]
pub struct SwSidebarArtEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub pixel_size: [f32; 2],
}

pub struct SwSidebarArt {
    pub texture: BatchTexture,
    entries: HashMap<SwSidebarArtKind, SwSidebarArtEntry>,
}

impl SwSidebarArt {
    pub fn entry(&self, kind: SwSidebarArtKind) -> Option<&SwSidebarArtEntry> {
        self.entries.get(&kind)
    }

    fn size(&self, kind: SwSidebarArtKind) -> Option<[i32; 2]> {
        self.entry(kind).map(|entry| {
            [
                entry.pixel_size[0].round() as i32,
                entry.pixel_size[1].round() as i32,
            ]
        })
    }

    pub fn top_size(&self) -> Option<[i32; 2]> {
        self.size(SwSidebarArtKind::Top)
    }

    pub fn bottom_size(&self) -> Option<[i32; 2]> {
        self.size(SwSidebarArtKind::Bottom)
    }

    /// Phobos sizes the toggle gadget from the larger loaded On/Off image.
    pub fn toggle_size(&self) -> Option<[i32; 2]> {
        let on = self.size(SwSidebarArtKind::On);
        let off = self.size(SwSidebarArtKind::Off);
        match (on, off) {
            (Some(a), Some(b)) => Some([a[0].max(b[0]), a[1].max(b[1])]),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }
}

pub fn build_sw_sidebar_art(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    asset_manager: &AssetManager,
    rules: &RuleSet,
    side_index: i32,
) -> Option<SwSidebarArt> {
    let art = rules.side_sw_sidebar_art(side_index)?;
    let images = [
        (SwSidebarArtKind::On, art.on_pcx.as_deref()),
        (SwSidebarArtKind::Off, art.off_pcx.as_deref()),
        (SwSidebarArtKind::Top, art.top_pcx.as_deref()),
        (SwSidebarArtKind::Center, art.center_pcx.as_deref()),
        (SwSidebarArtKind::Bottom, art.bottom_pcx.as_deref()),
    ]
    .into_iter()
    .filter_map(|(kind, name)| Some((kind, load_pcx(asset_manager, name?)?)))
    .collect();
    let (texture, entries) = pack_atlas(gpu, batch, images)?;
    log::info!("SuperWeaponSidebar art atlas: {} entries", entries.len());
    Some(SwSidebarArt { texture, entries })
}

fn load_pcx(asset_manager: &AssetManager, name: &str) -> Option<NativePcxImage> {
    let image = crate::render::sidebar_cameo_atlas::decode_native_pcx(asset_manager.get_ref(name)?);
    if image.is_none() {
        log::warn!("SuperWeaponSidebar PCX {name} did not load");
    }
    image
}

fn pack_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    images: Vec<(SwSidebarArtKind, NativePcxImage)>,
) -> Option<(BatchTexture, HashMap<SwSidebarArtKind, SwSidebarArtEntry>)> {
    if images.is_empty() {
        return None;
    }
    let width = images
        .iter()
        .map(|(_, image)| image.width)
        .max()
        .unwrap_or(1);
    let height = images
        .iter()
        .map(|(_, image)| image.height + ART_PADDING)
        .sum::<u32>()
        .saturating_sub(ART_PADDING)
        .max(1);
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let mut entries = HashMap::new();
    let mut y = 0u32;
    for (key, image) in images {
        for row in 0..image.height {
            let src = (row * image.width * 4) as usize;
            let dst = (((y + row) * width) * 4) as usize;
            rgba[dst..dst + (image.width * 4) as usize]
                .copy_from_slice(&image.rgba[src..src + (image.width * 4) as usize]);
        }
        entries.insert(
            key,
            SwSidebarArtEntry {
                uv_origin: [0.0, y as f32 / height as f32],
                uv_size: [
                    image.width as f32 / width as f32,
                    image.height as f32 / height as f32,
                ],
                pixel_size: [image.width as f32, image.height as f32],
            },
        );
        y += image.height + ART_PADDING;
    }
    Some((batch.create_texture(gpu, &rgba, width, height), entries))
}
