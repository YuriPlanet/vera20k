//! Art sources for the UI. Consumers try the active mod's candidates for a
//! supported slot and decode them before falling back to installation art.
//! Native paired RA2/YR slots can still name their exact style fallback.

use std::io::Cursor;
use std::path::Path;

use crate::assets::asset_manager::AssetManager;
use crate::assets::error::AssetError;
use crate::assets::fnt_file::FntFile;
use crate::assets::pcx_file::PcxFile;
use crate::assets::shp_file::ShpFile;

const MAX_UI_ART_BYTES: usize = 64 * 1024 * 1024;
const MAX_UI_ART_DIMENSION: u32 = 4096;
const MAX_UI_SHAPE_PIXELS: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub(crate) enum ShellUiAssetSource {
    Active,
    Archive(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ShellUiAssetLocator<'name> {
    pub(crate) member: &'name str,
    pub(crate) source: ShellUiAssetSource,
}

impl<'name> ShellUiAssetLocator<'name> {
    pub(crate) const fn active(member: &'name str) -> Self {
        Self {
            member,
            source: ShellUiAssetSource::Active,
        }
    }

    pub(crate) const fn archive(member: &'name str, archive: &'static str) -> Self {
        Self {
            member,
            source: ShellUiAssetSource::Archive(archive),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ShellUiAssetPair<'name> {
    pub(crate) ra2: ShellUiAssetLocator<'name>,
    pub(crate) yr: ShellUiAssetLocator<'name>,
}

impl<'name> ShellUiAssetPair<'name> {
    pub(crate) const fn new(
        ra2: ShellUiAssetLocator<'name>,
        yr: ShellUiAssetLocator<'name>,
    ) -> Self {
        Self { ra2, yr }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ShellUiAssetRef<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) source: &'a str,
}

pub(crate) fn mod_art_candidates<'a>(
    assets: &'a AssetManager,
    name: &str,
) -> impl Iterator<Item = ShellUiAssetRef<'a>> + use<'a> {
    assets
        .mod_asset_candidates(name, MAX_UI_ART_BYTES)
        .map(|(bytes, source)| ShellUiAssetRef { bytes, source })
}

pub(crate) fn base_art_candidates<'a>(
    assets: &'a AssetManager,
    name: &str,
) -> impl Iterator<Item = ShellUiAssetRef<'a>> + use<'a> {
    let has_mod = assets.mod_selected();
    let usual = if has_mod {
        None
    } else {
        assets
            .resolve_ref_bounded(name, MAX_UI_ART_BYTES)
            .ok()
            .flatten()
    };
    usual
        .into_iter()
        .map(|asset| ShellUiAssetRef {
            bytes: asset.bytes,
            source: asset.source_archive,
        })
        .chain(
            assets
                .base_asset_candidates(name, MAX_UI_ART_BYTES)
                .take(if has_mod { usize::MAX } else { 0 })
                .map(|(bytes, source)| ShellUiAssetRef { bytes, source }),
        )
}

pub(crate) fn ui_art_candidates<'a>(
    assets: &'a AssetManager,
    name: &str,
) -> impl Iterator<Item = ShellUiAssetRef<'a>> + use<'a> {
    mod_art_candidates(assets, name).chain(base_art_candidates(assets, name))
}

pub(crate) fn load_game_font(assets: &AssetManager) -> Option<FntFile> {
    ui_art_candidates(assets, "GAME.FNT").find_map(|asset| {
        FntFile::from_bytes(asset.bytes)
            .inspect_err(|error| {
                log::warn!("Could not load GAME.FNT from {}: {error}", asset.source)
            })
            .ok()
    })
}

pub(crate) fn decode_ui_shp(bytes: &[u8]) -> Result<ShpFile, AssetError> {
    let shp = ShpFile::from_bytes_bounded(bytes, MAX_UI_SHAPE_PIXELS)?;
    let fits = |width: u16, height: u16| u32::from(width.max(height)) <= MAX_UI_ART_DIMENSION;
    if !fits(shp.width, shp.height)
        || shp
            .frames
            .iter()
            .any(|frame| !fits(frame.frame_width, frame.frame_height))
    {
        return Err(AssetError::ParseError {
            format: "SHP".into(),
            detail: "UI shape dimensions are too large".into(),
        });
    }
    Ok(shp)
}

pub(crate) fn decode_ui_pcx(bytes: &[u8]) -> Result<PcxFile, AssetError> {
    let pcx = PcxFile::from_bytes_bounded(bytes, MAX_UI_ART_BYTES)?;
    if u32::from(pcx.width.max(pcx.height)) > MAX_UI_ART_DIMENSION {
        return Err(AssetError::ParseError {
            format: "PCX".into(),
            detail: "UI picture dimensions are too large".into(),
        });
    }
    Ok(pcx)
}

/// Decode an artwork path admitted by the client theme reader.
pub(crate) fn read_client_png(path: &Path) -> Option<image::RgbaImage> {
    let decode = || -> anyhow::Result<_> {
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let bytes = crate::util::file_system::read_shared(path)?;
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let bytes = {
            use std::io::Read;
            let file = std::fs::File::open(path)?;
            let mut bytes = Vec::new();
            file.take(MAX_UI_ART_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            bytes
        };
        anyhow::ensure!(
            bytes.len() <= MAX_UI_ART_BYTES,
            "UI picture file is too large"
        );
        let mut reader =
            image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_UI_ART_DIMENSION);
        limits.max_image_height = Some(MAX_UI_ART_DIMENSION);
        limits.max_alloc = Some(MAX_UI_ART_BYTES as u64);
        reader.limits(limits);
        Ok(reader.decode()?.into_rgba8())
    };
    decode()
        .inspect_err(|error| log::warn!("Could not load mod UI art {}: {error:#}", path.display()))
        .ok()
}

pub(crate) struct ShellUiAssetResolver<'a> {
    assets: &'a AssetManager,
    yr_original_style: bool,
}

impl<'a> ShellUiAssetResolver<'a> {
    pub(crate) const fn new(assets: &'a AssetManager, yr_original_style: bool) -> Self {
        Self {
            assets,
            yr_original_style,
        }
    }

    pub(crate) fn resolve(
        &self,
        pair: ShellUiAssetPair<'_>,
    ) -> anyhow::Result<ShellUiAssetRef<'a>> {
        let locator = if self.yr_original_style {
            pair.yr
        } else {
            pair.ra2
        };
        match locator.source {
            ShellUiAssetSource::Active => base_art_candidates(self.assets, locator.member)
                .next()
                .ok_or_else(|| {
                    anyhow::anyhow!("missing installation shell asset {}", locator.member)
                }),
            ShellUiAssetSource::Archive(archive_name) => {
                let (source, archive) =
                    self.assets
                        .archive_with_source(archive_name)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "missing shell style archive {archive_name} for {}",
                                locator.member
                            )
                        })?;
                let bytes = archive.get_by_name(locator.member).ok_or_else(|| {
                    anyhow::anyhow!("missing shell style member {} in {source}", locator.member)
                })?;
                Ok(ShellUiAssetRef { bytes, source })
            }
        }
    }
}
