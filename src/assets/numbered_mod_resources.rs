//! Recognize a selected mod that uses the classic RA2 resource family.
//!
//! This only proposes names. Startup still owns extension selection and uses
//! the ordinary asset/rules loaders. No executable or arbitrary MIX scan runs.

use std::path::Path;

use super::asset_manager::AssetManager;

/// An ordinary folder mod can supply the original RA2 files loose or in
/// EXPANDnn.MIX. Inspect only the mod: the installation itself contains
/// resources for both games. Mixed packages retain the existing YR choice.
pub(super) fn uses_classic_ra2(dir: &Path) -> Result<bool, String> {
    let mut assets = AssetManager::from_ini_probe_root(dir).map_err(|error| error.to_string())?;
    let has_root = |assets: &AssetManager, suffix: &str| {
        ["rules", "art", "ai", "mpmodes"]
            .iter()
            .any(|stem| assets.contains(&format!("{stem}{suffix}.ini")))
    };
    if has_root(&assets, "md")
        || (0..=99).any(|slot| assets.contains(&format!("expandmd{slot:02}.mix")))
    {
        return Ok(false);
    }
    for slot in (0..=99).rev() {
        let name = format!("expand{slot:02}.mix");
        if assets.contains(&name)
            && let Err(error) = assets.load_nested(&name)
        {
            log::warn!("Skipping unreadable classic archive {name}: {error}");
        }
    }
    Ok(has_root(&assets, "") && !has_root(&assets, "md"))
}
