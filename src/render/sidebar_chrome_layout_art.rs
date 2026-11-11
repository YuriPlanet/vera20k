//! Pictures a side's interface layout names for the buttons it adds
//! (`veraui.ini` `Art=`): any SHP the game can find, drawn with the generic
//! sidebar palette as the command bar's buttons are. Frame 0 is the button,
//! frame 1 the button held down.

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
            // A mod's own file first, then the side's archives.
            let bytes = route
                .asset_manager
                .get_ref(&name)
                .or_else(|| route.resolve(&name).map(|source| source.bytes));
            let shp = bytes.and_then(|bytes| ShpFile::from_bytes(bytes).ok());
            if shp.is_none() {
                log::warn!(
                    "{} Art={name}: no such picture; the button shows its label",
                    crate::ui::match_layout::FILE_NAME
                );
            }
            let frames = std::array::from_fn(|frame| {
                shp.as_ref().and_then(|shp| render_shp(shp, palette, frame))
            });
            (name, frames)
        })
        .collect();
    LayoutArt { pictures }
}

#[cfg(test)]
mod tests {
    use super::LayoutArt;

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
}
