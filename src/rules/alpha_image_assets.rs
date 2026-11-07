//! Scenario-load binding for inherited ObjectType AlphaImage resources.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::assets::asset_manager::AssetManager;
use crate::assets::shp_file::ShpFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;

/// One loaded AlphaImage SHP, as AlphaShapeClass reads it.
#[derive(Debug, PartialEq, Eq)]
pub struct AlphaImage {
    canvas: [i32; 2],
    frame_bounds: [i32; 4],
    pixels: Vec<u8>,
}

impl AlphaImage {
    /// The SHP header's width and height words (+2/+4), sign-extended: the
    /// AlphaShapeClass ctor's size (`0x004209B0`/`0x004209B4`), which
    /// `ObjectClass::Unlimbo` halves to centre the shape
    /// (`0x005F50C3..0x005F50DD`).
    pub fn canvas(&self) -> [i32; 2] {
        self.canvas
    }

    /// Frame 0's rectangle (`SHPStruct::GetFrameBounds @ 0x0069E7E0`).
    pub fn frame_bounds(&self) -> [i32; 4] {
        self.frame_bounds
    }

    /// Frame 0's bytes, row-major over [`Self::frame_bounds`]. Drawing reads
    /// them from the frame's data offset (`0x0069E740`) and never decodes
    /// RLE, so a compressed frame contributes its encoded bytes here too.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// A file that is not an SHP binds nothing. Frame bytes that are
    /// missing or end early, which gamemd would read past, draw nothing.
    fn from_shp(bytes: &[u8]) -> Option<Self> {
        let canvas = ShpFile::canvas_size_from_bytes(bytes)
            .ok()?
            .map(|word| i32::from(word as i16));
        let bounds = ShpFile::frame_bounds_from_bytes(bytes, 0).ok()?;
        let [_, _, width, height] = bounds.map(|value| usize::try_from(value).unwrap_or(0));
        let pixels = ShpFile::encoded_frame_data(bytes, 0)
            .ok()
            .and_then(|data| data.get(..width * height))
            .filter(|pixels| !pixels.is_empty());
        Some(match pixels {
            Some(pixels) => Self {
                canvas,
                frame_bounds: bounds,
                pixels: pixels.to_vec(),
            },
            None => Self {
                canvas,
                frame_bounds: [0; 4],
                pixels: Vec::new(),
            },
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(canvas: [i32; 2], frame_bounds: [i32; 4], pixels: Vec<u8>) -> Self {
        assert_eq!(pixels.len(), (frame_bounds[2] * frame_bounds[3]) as usize);
        Self {
            canvas,
            frame_bounds,
            pixels,
        }
    }
}

/// Every ObjectType `AlphaImage=` SHP that loaded, by the name its types hold.
///
/// This runs after scenario rules are folded and before scenario objects are
/// constructed. Native `ObjectTypeClass::ReadINI @ 0x005F92D0` and `Load
/// @ 0x005F9720` call `LoadFileFromMIX @ 0x005B40B0`; a failure leaves the
/// type's pointer (+0xAC) null, and such a type makes no AlphaShapeClass
/// (`crate::sim::world::alpha_shape`). Retail's values are ALPHATST (TSTLAMP)
/// and NONE (GALITE and the coloured lamps), and no retail archive holds
/// either SHP, so only maps and mods reach the glows.
#[derive(Debug, Clone, Default)]
pub struct AlphaImageCatalog {
    images: BTreeMap<String, Arc<AlphaImage>>,
}

impl AlphaImageCatalog {
    pub(crate) fn bind(
        rules: &RuleSet,
        overlays: &OverlayTypeRegistry,
        assets: &AssetManager,
    ) -> Self {
        let mut names = BTreeSet::new();
        rules.visit_alpha_image_names(overlays, |name| {
            if !name.is_empty() {
                names.insert(name.to_string());
            }
        });
        let images = names
            .into_iter()
            .filter_map(|name| {
                let load = assets.load_file_from_mix(&format!("{name}.SHP"))?;
                Some((name, Arc::new(AlphaImage::from_shp(&load.bytes)?)))
            })
            .collect();
        Self { images }
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Arc<AlphaImage>> {
        self.images.get(name)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn insert_for_test(&mut self, name: &str, image: AlphaImage) {
        self.images.insert(name.to_string(), Arc::new(image));
    }
}

#[cfg(test)]
mod tests {
    use super::{AlphaImage, AlphaImageCatalog};
    use crate::assets::asset_manager::{AssetManager, test_support::TestDirectory};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::rules::ruleset::RuleSet;

    /// One-frame SHP: a 61x31 canvas, frame 0 at (2,3) sized 2x2.
    fn shp(format: u8, payload: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        for word in [0u16, 61, 31, 1, 2, 3, 2, 2] {
            data.extend_from_slice(&word.to_le_bytes());
        }
        data.push(format);
        data.extend_from_slice(&[0; 11]);
        data.extend_from_slice(&32u32.to_le_bytes());
        data.extend_from_slice(payload);
        data
    }

    #[test]
    fn alpha_images_bind_the_canvas_and_frame_zero_raw_through_the_shared_loader() {
        let root = TestDirectory::new("object-alpha-image-binding");
        // An RLE frame (format 3) keeps its encoded bytes: DrawAll_NoMask
        // reads 0x0069E740's raw view.
        std::fs::write(root.path().join("PANEL.SHP"), shp(3, &[4, 0, 0, 2, 4, 0]))
            .expect("write AlphaImage SHP");
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let ini = IniFile::from_str(
            "[BuildingTypes]\n0=LIGHT\n[LIGHT]\nAlphaImage=PANEL\n\
             [OverlayTypes]\n0=CRATE\n[CRATE]\nAlphaImage=NONE\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("minimal AlphaImage rules");
        let overlays = OverlayTypeRegistry::from_ini(&ini, None);

        let catalog = AlphaImageCatalog::bind(&rules, &overlays, &assets);

        assert_eq!(
            catalog.get("PANEL").map(|image| &**image),
            Some(&AlphaImage::for_test(
                [61, 31],
                [2, 3, 2, 2],
                vec![4, 0, 0, 2]
            ))
        );
        assert!(catalog.get("NONE").is_none());
    }
}
