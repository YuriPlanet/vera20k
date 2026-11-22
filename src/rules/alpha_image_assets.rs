//! Scenario-load binding for inherited ObjectType AlphaImage resources.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::assets::asset_manager::AssetManager;
use crate::assets::shp_file::ShpFile;
use crate::rules::ruleset::RuleSet;

/// One loaded AlphaImage SHP, as AlphaShapeClass reads it.
#[derive(Debug, PartialEq, Eq)]
pub struct AlphaImage {
    canvas: [i32; 2],
    frames: Vec<AlphaFrame>,
    // Frame offsets can overlap. Keep one copy of the encoded SHP.
    data: Arc<[u8]>,
}

#[derive(Debug, PartialEq, Eq, Default)]
struct AlphaFrame {
    bounds: [i32; 4],
    pixels: std::ops::Range<usize>,
}

impl AlphaImage {
    /// The SHP header's width and height words (+2/+4), sign-extended: the
    /// AlphaShapeClass ctor's size (`0x004209B0`/`0x004209B4`), which
    /// `ObjectClass::Unlimbo` halves to centre the shape
    /// (`0x005F50C3..0x005F50DD`).
    pub fn canvas(&self) -> [i32; 2] {
        self.canvas
    }

    /// Ares Hooks.Alpha.cpp, 42146E/42152C and 4210AC/4211AC, replace
    /// the pushed zero frame for frame bounds and pixels; the stock SHP readers
    /// resume. Ares.dll's selector takes floor(log2(signed SHP count)) high
    /// bits of PrimaryFacing.Current (War of Dawn build 10043C80..10043D00).
    pub fn facing_frame(&self, facing: u16) -> usize {
        let count = self.frames.len() as i16;
        if count <= 1 {
            return 0;
        }
        usize::from(facing >> (16 - count.ilog2()))
    }

    /// The frame's rectangle (`SHPStruct::GetFrameBounds @ 0x0069E7E0`).
    pub fn frame_bounds(&self, index: usize) -> [i32; 4] {
        self.frames.get(index).map_or([0; 4], |frame| frame.bounds)
    }

    /// The frame's bytes, row-major over [`Self::frame_bounds`]. Drawing reads
    /// them from the frame's data offset (`0x0069E740`) and never decodes
    /// RLE, so a compressed frame contributes its encoded bytes here too.
    pub fn pixels(&self, index: usize) -> &[u8] {
        self.frames
            .get(index)
            .and_then(|frame| self.data.get(frame.pixels.clone()))
            .unwrap_or_default()
    }

    /// A file that is not an SHP binds nothing. Frame bytes that are
    /// missing or end early, which gamemd would read past, draw nothing.
    fn from_shp(data: Arc<[u8]>) -> Option<Self> {
        let canvas = ShpFile::canvas_size_from_bytes(&data)
            .ok()?
            .map(|word| i32::from(word as i16));
        let count = ShpFile::frame_count_from_bytes(&data).ok()?;
        let frames = (0..usize::from(count))
            .map(|index| {
                let bounds = ShpFile::frame_bounds_from_bytes(&data, index).unwrap_or([0; 4]);
                let [_, _, w, h] = bounds.map(|word| word.max(0) as usize);
                let size = w * h;
                if let Ok(tail) = ShpFile::encoded_frame_data(&data, index)
                    && size > 0
                    && size <= tail.len()
                {
                    let start = data.len() - tail.len();
                    AlphaFrame {
                        bounds,
                        pixels: start..start + size,
                    }
                } else {
                    AlphaFrame::default()
                }
            })
            .collect();
        Some(Self {
            canvas,
            frames,
            data,
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(canvas: [i32; 2], frame_bounds: [i32; 4], pixels: Vec<u8>) -> Self {
        Self::from_frames_for_test(canvas, &[(frame_bounds, &pixels)])
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn from_frames_for_test(canvas: [i32; 2], frames: &[([i32; 4], &[u8])]) -> Self {
        let mut data = vec![0u8; 8 + 24 * frames.len()];
        for (field, word) in data[2..8].chunks_exact_mut(2).zip([
            canvas[0] as u16,
            canvas[1] as u16,
            frames.len() as u16,
        ]) {
            field.copy_from_slice(&word.to_le_bytes());
        }
        for (i, (bounds, pixels)) in frames.iter().enumerate() {
            let offset = (data.len() as u32).to_le_bytes();
            let start = 8 + i * 24;
            for (field, word) in data[start..start + 8].chunks_exact_mut(2).zip(bounds) {
                field.copy_from_slice(&(*word as i16).to_le_bytes());
            }
            data[start + 20..start + 24].copy_from_slice(&offset);
            data.extend_from_slice(pixels);
        }
        Self::from_shp(data.into()).expect("test SHP")
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
    pub(crate) fn bind(rules: &RuleSet, assets: &AssetManager) -> Self {
        let mut names = BTreeSet::new();
        rules.visit_alpha_image_names(|name| {
            if !name.is_empty() {
                names.insert(name.to_string());
            }
        });
        let images = names
            .into_iter()
            .filter_map(|name| {
                let load = assets.load_file_from_mix(&format!("{name}.SHP"))?;
                Some((name, Arc::new(AlphaImage::from_shp(load.bytes)?)))
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

#[cfg(all(test, tests_rules_a_n))]
mod tests {
    use super::{AlphaImage, AlphaImageCatalog};
    use crate::assets::asset_manager::{AssetManager, test_support::TestDirectory};
    use crate::rules::ini_parser::IniFile;
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

        let catalog = AlphaImageCatalog::bind(&rules, &assets);

        let image = catalog.get("PANEL").unwrap();
        assert_eq!(image.canvas(), [61, 31]);
        assert_eq!(image.frame_bounds(0), [2, 3, 2, 2]);
        assert_eq!(image.pixels(0), [4, 0, 0, 2]);
        assert!(catalog.get("NONE").is_none());
    }

    #[test]
    fn directional_alpha_uses_high_hull_facing_bits_and_each_frames_header() {
        let image = AlphaImage::from_frames_for_test(
            [8, 8],
            &[
                ([0, 0, 1, 1], &[4]),
                ([3, 6, 2, 1], &[5, 6]),
                ([0, 0, 1, 1], &[7]),
            ],
        );
        // With three frames only the first two are addressable, with the
        // direction changing exactly halfway through the turn.
        assert_eq!(
            [0x7FFF, 0x8000, 0xFFFF].map(|direction| image.facing_frame(direction)),
            [0, 1, 1]
        );
        assert_eq!(image.frame_bounds(1), [3, 6, 2, 1]);
        assert_eq!(image.pixels(1), &[5, 6]);
    }
}
