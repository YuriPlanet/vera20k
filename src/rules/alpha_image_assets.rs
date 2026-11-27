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
    /// `floor(log2(frames))` of the header's frame count (`SHP+6`), 0 when
    /// that word is not positive as a signed short (Ares `100431A0`).
    facing_bits: u32,
    /// The first `2^facing_bits` frames: stock draws frame 0 only, Ares a
    /// Techno's facing frame ([`Self::facing_frame`]).
    frames: Vec<AlphaFrame>,
}

/// One AlphaImage frame as the glow draw reads it.
#[derive(Debug, PartialEq, Eq)]
struct AlphaFrame {
    bounds: [i32; 4],
    pixels: Vec<u8>,
}

impl AlphaFrame {
    /// Frame `index`'s rectangle and bytes. Bytes that are missing or end
    /// early, which gamemd would read past, draw nothing.
    fn read(bytes: &[u8], index: usize) -> Option<Self> {
        let bounds = ShpFile::frame_bounds_from_bytes(bytes, index).ok()?;
        let [_, _, width, height] = bounds.map(|value| usize::try_from(value).unwrap_or(0));
        let pixels = ShpFile::encoded_frame_data(bytes, index)
            .ok()
            .and_then(|data| data.get(..width * height))
            .filter(|pixels| !pixels.is_empty());
        Some(match pixels {
            Some(pixels) => Self {
                bounds,
                pixels: pixels.to_vec(),
            },
            None => Self::empty(),
        })
    }

    const fn empty() -> Self {
        Self {
            bounds: [0; 4],
            pixels: Vec::new(),
        }
    }
}

/// A frame the image does not keep.
static NO_FRAME: AlphaFrame = AlphaFrame::empty();

impl AlphaImage {
    /// The SHP header's width and height words (+2/+4), sign-extended: the
    /// AlphaShapeClass ctor's size (`0x004209B0`/`0x004209B4`), which
    /// `ObjectClass::Unlimbo` halves to centre the shape
    /// (`0x005F50C3..0x005F50DD`).
    pub fn canvas(&self) -> [i32; 2] {
        self.canvas
    }

    /// Frame `frame`'s rectangle (`SHPStruct::GetFrameBounds @ 0x0069E7E0`);
    /// a frame past those kept is empty.
    pub fn frame_bounds(&self, frame: u16) -> [i32; 4] {
        self.frame(frame).bounds
    }

    /// Frame `frame`'s bytes, row-major over [`Self::frame_bounds`]. Drawing
    /// reads them from the frame's data offset (`0x0069E740`) and never
    /// decodes RLE, so a compressed frame contributes its encoded bytes here
    /// too.
    pub fn pixels(&self, frame: u16) -> &[u8] {
        &self.frame(frame).pixels
    }

    fn frame(&self, frame: u16) -> &AlphaFrame {
        self.frames.get(usize::from(frame)).unwrap_or(&NO_FRAME)
    }

    /// The frame Ares draws for a Techno whose PrimaryFacing reads `facing`
    /// (`Ares.dll 100431A0`, from the `0x004210AC`/`0x004211AC` and
    /// `0x0042146E`/`0x0042152C` hooks): `facing >> (16 - k)` for the image's
    /// `k` facing bits, so a one-frame image keeps frame 0.
    pub fn facing_frame(&self, facing: u16) -> u16 {
        (u32::from(facing) >> (16 - self.facing_bits)) as u16
    }

    /// A file that is not an SHP binds nothing.
    fn from_shp(bytes: &[u8]) -> Option<Self> {
        let canvas = ShpFile::canvas_size_from_bytes(bytes)
            .ok()?
            .map(|word| i32::from(word as i16));
        let count = ShpFile::frame_count_from_bytes(bytes).ok()? as i16;
        let facing_bits =
            u16::try_from(count).map_or(0, |count| count.checked_ilog2().unwrap_or(0));
        let frames = (0..1usize << facing_bits)
            .map(|index| AlphaFrame::read(bytes, index))
            .collect::<Option<_>>()?;
        Some(Self {
            canvas,
            facing_bits,
            frames,
        })
    }

    /// VERA-only, experimental (`docs/picture-model-art.md`): frame 0 of a
    /// declared picture as an AlphaImage. Each game pixel (`PictureSize=`,
    /// else the picture's own size; the picture a whole multiple of it) is
    /// the mean grey level of its picture pixels at least half covered, used
    /// as the SHP byte is: 127 leaves light as it is, 255 doubles it, 0 is
    /// black. A game pixel less than half covered is 127. The frame is
    /// cropped to its drawn pixels as a stored SHP frame is; a picture with
    /// none draws nothing. None when it does not fit its canvas.
    fn from_picture(
        picture: &crate::assets::picture_file::PictureFrames,
        size: Option<[i32; 2]>,
    ) -> Option<Self> {
        const NEUTRAL: u8 = 127;
        let canvas = size.map_or([picture.width, picture.height], |size| {
            size.map(|side| side.unsigned_abs())
        });
        let scale = picture.width / canvas[0].max(1);
        if [canvas[0] * scale, canvas[1] * scale] != [picture.width, picture.height] {
            return None;
        }
        let rgba = picture.shrunk_region(0, scale, canvas)?;
        let word = |value: u32| i32::try_from(value).unwrap_or(i32::MAX);
        let drawn = |x: u32, y: u32| rgba[((y * canvas[0] + x) * 4 + 3) as usize] != 0;
        let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..canvas[1] {
            for x in 0..canvas[0] {
                if drawn(x, y) {
                    (left, top) = (left.min(x), top.min(y));
                    (right, bottom) = (right.max(x + 1), bottom.max(y + 1));
                }
            }
        }
        if left >= right {
            return Some(Self {
                canvas: canvas.map(word),
                facing_bits: 0,
                frames: vec![AlphaFrame {
                    bounds: [0; 4],
                    pixels: Vec::new(),
                }],
            });
        }
        let mut pixels = Vec::with_capacity(((right - left) * (bottom - top)) as usize);
        for y in top..bottom {
            for x in left..right {
                let at = ((y * canvas[0] + x) * 4) as usize;
                pixels.push(if drawn(x, y) {
                    ((u32::from(rgba[at]) + u32::from(rgba[at + 1]) + u32::from(rgba[at + 2]) + 1)
                        / 3) as u8
                } else {
                    NEUTRAL
                });
            }
        }
        Some(Self {
            canvas: canvas.map(word),
            facing_bits: 0,
            frames: vec![AlphaFrame {
                bounds: [
                    word(left),
                    word(top),
                    word(right - left),
                    word(bottom - top),
                ],
                pixels,
            }],
        })
    }

    /// A one-frame image.
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(canvas: [i32; 2], frame_bounds: [i32; 4], pixels: Vec<u8>) -> Self {
        Self::frames_for_test(canvas, vec![(frame_bounds, pixels)])
    }

    /// An image of `frames`, whose count must be a power of two.
    #[cfg(any(test, feature = "test-support"))]
    pub fn frames_for_test(canvas: [i32; 2], frames: Vec<([i32; 4], Vec<u8>)>) -> Self {
        assert!(frames.len().is_power_of_two());
        Self {
            canvas,
            facing_bits: frames.len().ilog2(),
            frames: frames
                .into_iter()
                .map(|(bounds, pixels)| {
                    assert_eq!(pixels.len(), (bounds[2] * bounds[3]) as usize);
                    AlphaFrame { bounds, pixels }
                })
                .collect(),
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
    /// VERA-only, experimental: the names whose art declares `Picture=`,
    /// and whether that picture loaded.
    pictures: BTreeMap<String, bool>,
    /// VERA: by image name, the game-folder file that supplied a loaded SHP
    /// where a retail install loads none (`retail_sources::FolderFiles`).
    folder_files: BTreeMap<String, String>,
}

impl AlphaImageCatalog {
    pub(crate) fn bind(rules: &RuleSet, assets: &AssetManager) -> Self {
        let mut names = BTreeSet::new();
        rules.visit_alpha_image_names(|name| {
            if !name.is_empty() {
                names.insert(name.to_string());
            }
        });
        let mut pictures = BTreeMap::new();
        let mut folder_files = BTreeMap::new();
        let folder = crate::rules::retail_sources::FolderFiles::new(assets);
        let images = names
            .into_iter()
            .filter_map(|name| {
                // VERA-only (docs/picture-model-art.md): an AlphaImage whose
                // art section declares `Picture=` is read from the picture;
                // its SHP is never opened.
                if let Some((section, decl)) = rules.art().resolve_declared_picture(&name, "") {
                    let image = rules
                        .art()
                        .picture_candidates(section, decl, "")
                        .iter()
                        .find_map(|file| assets.get_ref(file))
                        .and_then(|bytes| {
                            crate::assets::picture_file::PictureFrames::decode(bytes).ok()
                        })
                        .and_then(|picture| AlphaImage::from_picture(&picture, decl.size));
                    if image.is_none() {
                        log::warn!("[{section}] AlphaImage picture {} cannot be drawn", decl.file);
                        crate::rules::art_problems::report(
                            section,
                            &format!("Picture={}", decl.file),
                            "the picture is missing, unreadable or not a whole multiple of PictureSize=",
                            "the AlphaImage is not loaded",
                        );
                    }
                    pictures.insert(name.clone(), image.is_some());
                    return Some((name, Arc::new(image?)));
                }
                let file = format!("{name}.SHP");
                let load = assets.load_file_from_mix(&file)?;
                let image = Arc::new(AlphaImage::from_shp(&load.bytes)?);
                let shipped_loads = || {
                    folder
                        .shipped(&file)
                        .is_some_and(|bytes| AlphaImage::from_shp(bytes).is_some())
                };
                if !crate::rules::retail_sources::shipped_source(&load.source_archive)
                    && !shipped_loads()
                {
                    folder_files.insert(
                        name.clone(),
                        crate::rules::retail_sources::game_folder_file(&load.source_archive),
                    );
                }
                Some((name, image))
            })
            .collect();
        Self {
            images,
            pictures,
            folder_files,
        }
    }

    /// VERA: the game-folder file that makes the SHP of `name` load.
    pub(crate) fn folder_file(&self, name: &str) -> Option<&str> {
        self.folder_files.get(name).map(String::as_str)
    }

    /// VERA-only, experimental (`docs/picture-model-art.md`): what the
    /// simulation takes from declared AlphaImage pictures, for the config
    /// hash: whether each loaded (Ares particle systems read it,
    /// `AresParticleSystemExt::for_system`) and its canvas (the glow's
    /// centre). Nothing is hashed when no AlphaImage declares a picture, so
    /// the hash of rules without one is unchanged.
    pub(crate) fn hash_declared_pictures<H: std::hash::Hasher>(&self, state: &mut H) {
        use std::hash::Hash;
        if self.pictures.is_empty() {
            return;
        }
        b"alpha-image-pictures-v1".hash(state);
        for (name, &loaded) in &self.pictures {
            name.hash(state);
            loaded.hash(state);
            if let Some(image) = self.images.get(name) {
                image.canvas.hash(state);
            }
        }
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Arc<AlphaImage>> {
        self.images.get(name)
    }

    /// Whether the SHP of `name` loaded.
    pub(crate) fn loaded(&self, name: &str) -> bool {
        self.images.contains_key(name)
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

    /// VERA-only (docs/picture-model-art.md): an AlphaImage whose art
    /// declares `Picture=` is read from the picture, shrunk from 2x: grey
    /// levels as SHP bytes, cropped to the drawn pixels, a hole inside the
    /// crop neutral (127); its SHP is never opened. The config hash takes it.
    #[test]
    fn a_declared_alpha_picture_binds_grey_levels_and_enters_the_hash() {
        let root = TestDirectory::new("object-alpha-image-picture");
        std::fs::write(root.path().join("PANEL.SHP"), shp(3, &[4, 0, 0, 2, 4, 0]))
            .expect("write AlphaImage SHP");
        // 8x4 at 2x: game pixels (1,0) grey 200 and (3,1) grey 60, the rest
        // transparent.
        let mut rgba = vec![0u8; 8 * 4 * 4];
        for (x, y, grey) in [(2, 0, 200u8), (3, 0, 200), (2, 1, 200), (3, 1, 200)]
            .into_iter()
            .chain([(6, 2, 60u8), (7, 2, 60), (6, 3, 60), (7, 3, 60)])
        {
            let at = (y * 8 + x) * 4;
            rgba[at..at + 4].copy_from_slice(&[grey, grey, grey, 255]);
        }
        std::fs::write(
            root.path().join("panel.png"),
            crate::assets::picture_file::encode_frames(8, 4, &[rgba]).unwrap(),
        )
        .expect("write AlphaImage picture");
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let ini = IniFile::from_str("[BuildingTypes]\n0=LIGHT\n[LIGHT]\nAlphaImage=PANEL\n");
        let mut rules = RuleSet::from_ini(&ini).expect("minimal AlphaImage rules");
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(
            &IniFile::from_str("[PANEL]\nPicture=panel.png\nPictureSize=4,2\n"),
        ));
        let unbound_hash = rules.simulation_config_hash();
        rules.bind_alpha_images(&assets);

        assert_eq!(
            rules.alpha_image("PANEL").map(|image| &**image),
            Some(&AlphaImage::for_test(
                [4, 2],
                [1, 0, 3, 2],
                vec![200, 127, 127, 127, 127, 60]
            ))
        );
        assert_ne!(rules.simulation_config_hash(), unbound_hash);
    }
}
