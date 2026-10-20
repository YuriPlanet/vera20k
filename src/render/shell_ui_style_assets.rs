//! Explicit source selection for shell assets with a confirmed RA2/YR variant.
//!
//! Shared and YR-only resources continue through `AssetManager`'s ordinary
//! lookup. A RA2 style choice names its exact archive and member so a missing
//! RA2 copy is diagnosable instead of silently resolving the active YR asset.

use crate::assets::asset_manager::AssetManager;

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
            ShellUiAssetSource::Active => {
                let (bytes, source) =
                    self.assets
                        .get_with_source_ref(locator.member)
                        .ok_or_else(|| {
                            anyhow::anyhow!("missing active shell asset {}", locator.member)
                        })?;
                Ok(ShellUiAssetRef { bytes, source })
            }
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
