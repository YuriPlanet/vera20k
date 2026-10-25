//! Scenario-load binding for inherited ObjectType AlphaImage resources.

use std::collections::BTreeSet;

use crate::assets::asset_manager::AssetManager;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AlphaImageBindingSummary {
    pub(crate) attempted: usize,
    pub(crate) loaded: usize,
}

/// Load retained ObjectType `AlphaImage` SHPs through the shared asset owner.
///
/// This runs after scenario rules are folded and before scenario objects are
/// constructed. Native `ObjectTypeClass::ReadINI @ 0x005F92D0` and `Load
/// @ 0x005F9720` call `LoadFileFromMIX @ 0x005B40B0`; failures leave the native
/// pointer null, so missing files remain a soft failure here as well. The Rust
/// types retain the final strings in their existing owners; this scenario pass
/// visits those admitted values and reuses AssetManager's sticky cache without
/// adding drawing state or an SHP decoder.
pub fn bind(rules: &RuleSet, overlays: &OverlayTypeRegistry, assets: &AssetManager) {
    let _ = bind_with_summary(rules, overlays, assets);
}

fn bind_with_summary(
    rules: &RuleSet,
    overlays: &OverlayTypeRegistry,
    assets: &AssetManager,
) -> AlphaImageBindingSummary {
    let mut filenames = BTreeSet::new();
    rules.visit_alpha_image_names(overlays, |name| {
        if !name.is_empty() {
            filenames.insert(format!("{name}.SHP"));
        }
    });

    let attempted = filenames.len();
    let loaded = filenames
        .iter()
        .filter(|filename| assets.load_file_from_mix(filename.as_str()).is_some())
        .count();
    AlphaImageBindingSummary { attempted, loaded }
}

#[cfg(test)]
mod tests {
    use super::bind_with_summary as bind;
    use crate::assets::asset_manager::{AssetManager, test_support::TestDirectory};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::rules::ruleset::RuleSet;

    #[test]
    fn alpha_image_binding_uses_the_shared_file_loader_for_name_and_none() {
        let root = TestDirectory::new("object-alpha-image-binding");
        std::fs::write(root.path().join("PANEL.SHP"), b"loose SHP bytes")
            .expect("write AlphaImage SHP");
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let ini = IniFile::from_str(
            "[BuildingTypes]\n0=LIGHT\n[LIGHT]\nAlphaImage=PANEL\n\
             [OverlayTypes]\n0=CRATE\n[CRATE]\nAlphaImage=NONE\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("minimal AlphaImage rules");
        let overlays = OverlayTypeRegistry::from_ini(&ini, None);

        let summary = bind(&rules, &overlays, &assets);

        assert_eq!(summary.attempted, 2);
        assert_eq!(summary.loaded, 1);
    }
}
