//! Pictures a side's interface layout names for the buttons it adds
//! (`veraui.ini` `Art=`): any SHP the game can find, drawn with the generic
//! sidebar palette as the command bar's buttons are, or a PCX with its own
//! palette. Frame 0 is the button, frame 1 the button held down.
//!
//! The client modules' nodes and the map's `[VeraUI]` parts name theirs the
//! same way, at any time (RFC 0001 appendix 7.10 `Art`): [`NamedPictures`]
//! finds each the first time it is named and packs them in a texture of
//! their own, apart from the sidebar's atlas.

use super::*;

/// The pictures of the layout file's added buttons, by file name.
pub struct LayoutArt<T> {
    pub pictures: Vec<(String, [Option<T>; 2])>,
}

impl<T> LayoutArt<T> {
    pub fn entries(&self) -> impl Iterator<Item = &T> {
        self.pictures
            .iter()
            .flat_map(|(_, frames)| frames.iter().flatten())
    }

    pub fn map<U>(&self, mut f: impl FnMut(&T) -> U) -> LayoutArt<U> {
        LayoutArt {
            pictures: self
                .pictures
                .iter()
                .map(|(name, frames)| {
                    (
                        name.clone(),
                        frames.each_ref().map(|entry| entry.as_ref().map(&mut f)),
                    )
                })
                .collect(),
        }
    }

    /// A picture's frames by the name a layout wrote, without regard to
    /// capitals: the button, and the button held down (the same picture when
    /// the file has one frame). `None` when the file was not found.
    pub fn frames(&self, name: &str) -> Option<[&T; 2]> {
        let (_, [up, down]) = self
            .pictures
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))?;
        let up = up.as_ref()?;
        Some([up, down.as_ref().unwrap_or(up)])
    }
}

pub(super) fn load(
    route: SidebarSideRoute<'_>,
    palette: &Palette,
) -> LayoutArt<RenderedChromeEntry> {
    // The layout file is read here as UIMD.INI is for the command bar: the
    // names it gives decide which pictures the atlas holds.
    let names = route
        .asset_manager
        .get_ref(crate::ui::match_layout::FILE_NAME)
        .map(crate::ui::match_layout::MatchLayoutFile::from_bytes)
        .map(|file| file.art_names())
        .unwrap_or_default();
    let pictures = names
        .into_iter()
        .map(|name| {
            let frames = render_named(route, palette, &name);
            if frames[0].is_none() {
                log::warn!(
                    "{} Art={name}: no such picture; the button shows its label",
                    crate::ui::match_layout::FILE_NAME
                );
            }
            (name, frames)
        })
        .collect();
    LayoutArt { pictures }
}

/// Picture `name`'s two frames, the button and the button held down (`None`
/// where the file lacks one; both when no file is found). A `.pcx` is one
/// frame in its own palette, its magenta clear (as a PCX cameo); any other
/// name is an SHP in `palette`, the mod's own file first, then the side's
/// archives.
fn render_named(
    route: SidebarSideRoute<'_>,
    palette: &Palette,
    name: &str,
) -> [Option<RenderedChromeEntry>; 2] {
    let is_pcx = name
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("pcx"));
    if is_pcx {
        let up = crate::render::sidebar_cameo_atlas::load_native_pcx(route.asset_manager, name)
            .map(|image| RenderedChromeEntry {
                indices: vec![0; (image.width * image.height) as usize],
                rgba: image.rgba,
                width: image.width,
                height: image.height,
            });
        return [up, None];
    }
    let decode = |asset: ShellUiAssetRef<'_>| parse_sidebar_shape(route.asset_manager, asset.bytes);
    let shp = ui_art_candidates(route.asset_manager, name)
        .find_map(decode)
        .or_else(|| route.decode(name, decode));
    std::array::from_fn(|frame| shp.as_ref().and_then(|shp| render_shp(shp, palette, frame)))
}

/// Where [`NamedPictures`] finds its files: the side's archives and palette
/// the sidebar draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PictureSource {
    /// The theme of the sidebar atlas the local side draws
    /// (`ResolvedSidebarChrome::actual_theme`).
    pub theme: SidebarTheme,
    /// The Ares arm's registered Side MIX group rather than a stock theme's
    /// archives ([`SidebarChromeSet::ares`]).
    pub ares: bool,
}

/// The pictures client nodes name (`Art`), each found by its name the first
/// time it is named and kept, found or not: a name no file answers draws
/// nothing and is not looked for again. They are packed one under another
/// in one texture, made again on the frame a new name is found; a frame
/// that names nothing new costs nothing.
#[derive(Default)]
pub struct NamedPictures {
    /// The archives and palette they were drawn from.
    source: Option<PictureSource>,
    /// Every name asked for, in the order asked, with its frames.
    rendered: LayoutArt<RenderedChromeEntry>,
    /// The same frames' places in [`NamedPictures::texture`].
    packed: LayoutArt<SidebarChromeEntry>,
    texture: Option<BatchTexture>,
}

impl<T> Default for LayoutArt<T> {
    fn default() -> Self {
        Self {
            pictures: Vec::new(),
        }
    }
}

impl NamedPictures {
    /// The pictures' frames by name, in [`NamedPictures::texture`].
    pub fn art(&self) -> &LayoutArt<SidebarChromeEntry> {
        &self.packed
    }

    /// The texture they are packed in; `None` before one is found.
    pub fn texture(&self) -> Option<&BatchTexture> {
        self.texture.as_ref()
    }

    /// Find the `names` not asked for yet, drawn from `source`, and make the
    /// texture again when one was found. A new `source` (the local side
    /// changed to a side drawn from other archives) draws every name again.
    pub fn ask(
        &mut self,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        asset_manager: &AssetManager,
        source: PictureSource,
        names: impl IntoIterator<Item = String>,
    ) {
        let found = self.render(asset_manager, source, names);
        if !found {
            return;
        }
        let (rgba, [width, height], packed) = pack(&self.rendered);
        self.packed = packed;
        self.texture = Some(batch.create_texture(gpu, &rgba, width, height));
    }

    /// Draw the new names' frames; true when the texture must be made
    /// again (a picture was found, or the source changed).
    fn render(
        &mut self,
        asset_manager: &AssetManager,
        source: PictureSource,
        names: impl IntoIterator<Item = String>,
    ) -> bool {
        let mut names: Vec<String> = names.into_iter().collect();
        let redraw = self.source.is_some_and(|known| known != source);
        if redraw {
            let known = std::mem::take(&mut self.rendered.pictures);
            names.splice(0..0, known.into_iter().map(|(name, _)| name));
        }
        self.source = Some(source);
        names.retain(|name| !name.is_empty());
        let fresh: Vec<String> = names
            .into_iter()
            .filter(|name| self.rendered.frames_named(name).is_none())
            .fold(Vec::new(), |mut fresh, name| {
                if !fresh
                    .iter()
                    .any(|seen: &String| seen.eq_ignore_ascii_case(&name))
                {
                    fresh.push(name);
                }
                fresh
            });
        if fresh.is_empty() {
            return redraw;
        }
        let route = if source.ares {
            SidebarSideRoute::for_registered_group(asset_manager)
        } else {
            SidebarSideRoute::for_theme(asset_manager, source.theme)
        };
        let palette = route.decode(SIDEBAR_PALETTE, |asset| {
            decode_sidebar_palette(asset.bytes).ok()
        });
        let mut found = redraw;
        for name in fresh {
            let frames = match palette.as_ref() {
                Some(palette) => render_named(route, palette, &name),
                None => [None, None],
            };
            if frames[0].is_some() {
                found = true;
            } else {
                log::warn!("Art={name}: no such picture; the node shows none");
            }
            self.rendered.pictures.push((name, frames));
        }
        found
    }
}

impl<T> LayoutArt<T> {
    /// The entry of `name`, found or not, without regard to capitals.
    fn frames_named(&self, name: &str) -> Option<&[Option<T>; 2]> {
        self.pictures
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))
            .map(|(_, frames)| frames)
    }
}

/// `rendered`'s frames one under another in one picture, as the sidebar's
/// atlas packs its own: the RGBA, its size and each frame's place.
fn pack(
    rendered: &LayoutArt<RenderedChromeEntry>,
) -> (Vec<u8>, [u32; 2], LayoutArt<SidebarChromeEntry>) {
    let width = rendered
        .entries()
        .map(|e| e.width)
        .max()
        .unwrap_or(1)
        .max(1);
    let height = rendered
        .entries()
        .map(|e| e.height + CHROME_PADDING)
        .sum::<u32>()
        .max(1);
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let mut y = 0u32;
    let packed = rendered.map(|entry| {
        let uv = blit_entry(&mut rgba, width, height, y, entry);
        y += entry.height + CHROME_PADDING;
        uv
    });
    (rgba, [width, height], packed)
}

#[cfg(all(test, tests_render_s_z))]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_found_by_name_and_one_frame_serves_both_states() {
        let art = LayoutArt {
            pictures: vec![
                ("One.shp".to_owned(), [Some(1), None]),
                ("Two.SHP".to_owned(), [Some(2), Some(3)]),
                ("gone.shp".to_owned(), [None, None]),
            ],
        };
        assert_eq!(art.frames("one.SHP"), Some([&1, &1]));
        assert_eq!(art.frames("two.shp"), Some([&2, &3]));
        // A file that was not found, or was never named, has no picture.
        assert_eq!(art.frames("gone.shp"), None);
        assert_eq!(art.frames("other.shp"), None);
        assert_eq!(art.entries().count(), 3);
    }

    fn entry(width: u32, height: u32, value: u8) -> RenderedChromeEntry {
        RenderedChromeEntry {
            rgba: vec![value; (width * height * 4) as usize],
            indices: vec![value; (width * height) as usize],
            width,
            height,
        }
    }

    /// Named pictures pack one under another, padded, at their own sizes;
    /// one not found takes no room.
    #[test]
    fn named_pictures_pack_one_under_another() {
        let rendered = LayoutArt {
            pictures: vec![
                (
                    "a.shp".to_owned(),
                    [Some(entry(4, 2, 1)), Some(entry(4, 2, 2))],
                ),
                ("gone.shp".to_owned(), [None, None]),
                ("b.pcx".to_owned(), [Some(entry(2, 3, 3)), None]),
            ],
        };
        let (rgba, [width, height], packed) = pack(&rendered);
        assert_eq!([width, height], [4, 2 + 2 + 2 + 2 + 3 + 2]);
        assert_eq!(rgba.len(), (width * height * 4) as usize);
        let [up, down] = packed.frames("A.SHP").unwrap();
        assert_eq!(up.pixel_size, [4.0, 2.0]);
        assert_eq!(up.uv_origin, [0.0, 0.0]);
        assert_eq!(down.uv_origin, [0.0, 4.0 / height as f32]);
        let [one, same] = packed.frames("b.pcx").unwrap();
        assert_eq!(one.pixel_size, [2.0, 3.0]);
        assert_eq!(one.uv_origin, same.uv_origin);
        assert!(packed.frames("gone.shp").is_none());
        // Row 8 is b.pcx's first.
        assert_eq!(rgba[(8 * width * 4) as usize], 3);
    }

    /// A name is looked for once: found or not, asking again finds it in
    /// the cache, whatever its capitals; a new source draws them all again.
    #[test]
    fn a_name_is_looked_for_once_per_source() {
        let install =
            crate::assets::asset_manager::test_support::audio_test_install("named-pictures");
        let assets = AssetManager::new(
            install.path(),
            crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();
        let mut pictures = NamedPictures::default();
        let allied = PictureSource {
            theme: SidebarTheme::Allied,
            ares: false,
        };
        // Nothing to find: no texture is made, the names stay unknown.
        assert!(!pictures.render(&assets, allied, ["x.shp".to_owned(), String::new()]));
        assert!(!pictures.render(&assets, allied, ["X.SHP".to_owned(), "y.pcx".to_owned()]));
        let names: Vec<_> = pictures
            .rendered
            .pictures
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["x.shp", "y.pcx"]);
        assert!(!pictures.render(&assets, allied, std::iter::empty()));
        let soviet = PictureSource {
            theme: SidebarTheme::Soviet,
            ares: false,
        };
        assert!(pictures.render(&assets, soviet, std::iter::empty()));
        assert_eq!(pictures.rendered.pictures.len(), 2);
    }
}
