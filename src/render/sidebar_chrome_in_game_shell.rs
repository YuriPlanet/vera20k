//! Active-game dialog art uses the ordinary side MIX stack and SIDEBAR.PAL.
//!
//! Original loader 0x0072FA10 resolves SIDEBTTN.SHP via 0x00844CFC to
//! 0x00B0F9EC (0x0072FAC4..0x0072FAD4), and SIDE2B.SHP via 0x00844D20
//! to 0x00B0FA00 (0x0072FB2D..0x0072FB3D). Both paint through the converter
//! built from SIDEBAR.PAL at 0x0072FC1F..0x0072FC2F, including Yuri.

use super::{Palette, RenderedChromeEntry, SidebarSideRoute, render_side_entry};

/// Backgrounds use a separate converter from both radar and generic column art.
/// Original72FBC0: side2 uses844BF8→UIBKGDY.PAL; other sides use844BF4→UIBKGD.PAL,
/// constructing B0FBF0, which72F540 uses for BKGD*.SHP. Keep the selected source
/// beside the decoded entries so production provenance describes these pixels.
pub(super) fn load_backgrounds(
    assets: &super::AssetManager,
    mix: &super::MixArchive,
    mix_name: &str,
    names: (&str, &str, &str),
    palette_name: &str,
) -> Option<(
    [Option<RenderedChromeEntry>; 3],
    super::SidebarChromeAssetIdentity,
)> {
    let (palette, source_archive) =
        super::resolve_theme_palette_with_source(assets, mix, mix_name, palette_name)?;
    let entries =
        [names.0, names.1, names.2].map(|name| super::render_entry(assets, mix, name, &palette, 0));
    Some((
        entries,
        super::SidebarChromeAssetIdentity {
            logical_name: palette_name.to_string(),
            source_archive: Some(source_archive),
        },
    ))
}

/// Art shared by the pause menu and Game Controls in an active scenario.
pub struct InGameShellArt<T> {
    /// SIDE2B frame zero paints the blank button-column tiles. Native layout
    /// still advances by SIDE2's height (0x0072FD96 versus 0x0072F754).
    pub panel_tile: Option<T>,
    /// SIDEBTTN frames: released 0, pressed 1, timer highlight 2.
    /// Original type-2 branch: 0x00612EE8..0x00612F5B.
    pub buttons: [Option<T>; 3],
}

impl<T> InGameShellArt<T> {
    pub fn entries(&self) -> impl Iterator<Item = &T> {
        self.panel_tile.iter().chain(self.buttons.iter().flatten())
    }

    pub fn map<U>(&self, mut f: impl FnMut(&T) -> U) -> InGameShellArt<U> {
        InGameShellArt {
            panel_tile: self.panel_tile.as_ref().map(&mut f),
            buttons: self
                .buttons
                .each_ref()
                .map(|entry| entry.as_ref().map(&mut f)),
        }
    }
}

pub(super) fn load(
    route: SidebarSideRoute<'_>,
    palette: &Palette,
) -> InGameShellArt<RenderedChromeEntry> {
    InGameShellArt {
        panel_tile: render_side_entry(route, "SIDE2B.SHP", palette, 0),
        buttons: std::array::from_fn(|frame| {
            render_side_entry(route, "SIDEBTTN.SHP", palette, frame)
        }),
    }
}
